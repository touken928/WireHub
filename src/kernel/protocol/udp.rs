use super::*;
use crate::kernel::checksum::{read_u16, transport_valid};
use std::time::Duration;

pub(in crate::kernel) const UDP_IDLE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
pub(in crate::kernel) struct UdpPacket {
    src_port: u16,
    dst_port: u16,
}

impl UdpPacket {
    pub(super) fn parse(src: Ipv4Addr, dst: Ipv4Addr, bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || read_u16(bytes, 4) as usize != bytes.len() {
            return None;
        }
        if read_u16(bytes, 6) != 0 && !transport_valid(src, dst, 17, bytes) {
            return None;
        }
        Some(Self {
            src_port: read_u16(bytes, 0),
            dst_port: read_u16(bytes, 2),
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
            event: ConnectionEvent::Udp,
        })
    }
    pub(super) fn rewrite(
        &self,
        bytes: &mut [u8],
        plan: RewritePlan,
    ) -> Option<(Ipv4Addr, Ipv4Addr)> {
        rewrite_ports(bytes, plan, 17, 6, true)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpFlowState;

impl UdpFlowState {
    pub(super) fn is_closing(self) -> bool {
        false
    }
    pub(super) fn deadline(self, last: Instant) -> Instant {
        last + UDP_IDLE
    }
    pub(super) fn on_delivered(&mut self, _now: Instant) {}
}
