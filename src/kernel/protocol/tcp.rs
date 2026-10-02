use super::*;
use crate::kernel::checksum::{read_u16, transport_valid};
use std::time::Duration;

pub(in crate::kernel) const TCP_ESTABLISHED_IDLE: Duration =
    Duration::from_secs(2 * 60 * 60 + 4 * 60);
pub(in crate::kernel) const TCP_HANDSHAKE_IDLE: Duration = Duration::from_secs(60);
pub(in crate::kernel) const TCP_CLOSED_GRACE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
pub(in crate::kernel) struct TcpPacket {
    src_port: u16,
    dst_port: u16,
    segment: TcpSegment,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TcpSegment {
    pub flags: u8,
    pub seq: u32,
    pub ack: u32,
    pub payload_len: u32,
}

impl TcpSegment {
    pub(super) fn initial_syn(self) -> bool {
        self.flags & 2 != 0 && self.flags & 16 == 0
    }
}

impl TcpPacket {
    pub(super) fn parse(src: Ipv4Addr, dst: Ipv4Addr, bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 20 {
            return None;
        }
        let header_len = (bytes[12] >> 4) as usize * 4;
        if header_len < 20 || header_len > bytes.len() || !transport_valid(src, dst, 6, bytes) {
            return None;
        }
        Some(Self {
            src_port: read_u16(bytes, 0),
            dst_port: read_u16(bytes, 2),
            segment: TcpSegment {
                flags: bytes[13],
                seq: u32::from_be_bytes(bytes[4..8].try_into().ok()?),
                ack: u32::from_be_bytes(bytes[8..12].try_into().ok()?),
                payload_len: (bytes.len() - header_len) as u32,
            },
        })
    }
    pub(super) fn association(&self, src: Ipv4Addr, dst: Ipv4Addr) -> FlowAssociation {
        FlowAssociation::Connection(Connection {
            tuple: PacketTuple {
                src,
                dst,
                src_port: self.src_port,
                dst_port: self.dst_port,
            },
            event: ConnectionEvent::Tcp(self.segment),
        })
    }
    pub(super) fn rewrite(
        &self,
        bytes: &mut [u8],
        plan: RewritePlan,
    ) -> Option<(Ipv4Addr, Ipv4Addr)> {
        rewrite_ports(bytes, plan, 6, 16, false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::kernel) enum TcpState {
    SynSent {
        initiator_next: u32,
        initiator_end: u32,
    },
    SynReceived {
        initiator_next: u32,
        responder_next: u32,
        responder_end: u32,
    },
    Established,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TcpFlowState {
    pub(super) handshake: TcpState,
    pub(super) fin_directions: u8,
    pub(super) closed_until: Option<Instant>,
}

impl TcpFlowState {
    pub(super) fn new(segment: TcpSegment) -> Self {
        Self {
            handshake: TcpState::SynSent {
                initiator_next: segment.seq.wrapping_add(1),
                initiator_end: segment
                    .seq
                    .wrapping_add(1)
                    .wrapping_add(segment.payload_len),
            },
            fin_directions: 0,
            closed_until: None,
        }
    }
    fn close(&mut self, now: Instant) {
        let deadline = now + TCP_CLOSED_GRACE;
        self.closed_until = Some(
            self.closed_until
                .map_or(deadline, |current| current.min(deadline)),
        );
    }
}

impl TcpFlowState {
    pub(super) fn is_closing(self) -> bool {
        self.fin_directions != 0 || self.closed_until.is_some()
    }
    pub(super) fn deadline(self, last: Instant) -> Instant {
        self.closed_until.unwrap_or(
            last + if self.handshake == TcpState::Established {
                TCP_ESTABLISHED_IDLE
            } else {
                TCP_HANDSHAKE_IDLE
            },
        )
    }
    pub(super) fn on_delivered(&mut self, tcp: TcpSegment, from_initiator: bool, now: Instant) {
        if tcp.flags & (0x01 | 0x04) == 0 {
            self.handshake = match self.handshake {
                // TCP Fast Open may be accepted or declined; ACK must cover the SYN.
                TcpState::SynSent {
                    initiator_next,
                    initiator_end,
                } if !from_initiator
                    && tcp.flags & 0x12 == 0x12
                    && sequence_between(tcp.ack, initiator_next, initiator_end) =>
                {
                    TcpState::SynReceived {
                        initiator_next: tcp.ack,
                        responder_next: tcp.seq.wrapping_add(1),
                        responder_end: tcp.seq.wrapping_add(1).wrapping_add(tcp.payload_len),
                    }
                }
                // A later data/ACK can establish state if the first ACK was lost.
                TcpState::SynReceived {
                    initiator_next,
                    responder_next,
                    responder_end,
                } if from_initiator
                    && tcp.flags & 0x12 == 0x10
                    && (tcp.seq.wrapping_sub(initiator_next) as i32) >= 0
                    && sequence_between(tcp.ack, responder_next, responder_end) =>
                {
                    TcpState::Established
                }
                current => current,
            };
        }
        if tcp.flags & 0x04 != 0 {
            self.close(now);
        } else if tcp.flags & 0x01 != 0 {
            self.fin_directions |= if from_initiator { 1 } else { 2 };
            if self.fin_directions == 3 {
                self.close(now);
            }
        }
    }
}

fn sequence_between(value: u32, start: u32, end: u32) -> bool {
    value.wrapping_sub(start) <= end.wrapping_sub(start)
}
