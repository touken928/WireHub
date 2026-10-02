//! Static protocol dispatch. The router only sees associations and rewrite plans.
use std::{net::Ipv4Addr, time::Instant};
use super::checksum::transport_checksum;

mod tcp;
mod udp;
mod icmp;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PacketTuple {
    pub src: Ipv4Addr,
    pub src_port: u16,
    pub dst: Ipv4Addr,
    pub dst_port: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FlowAssociation {
    Connection { protocol: u8, tuple: PacketTuple, event: FlowEvent },
    /// ICMP errors can use an existing mapping, but cannot create or refresh one.
    Related { protocol: u8, tuple: PacketTuple },
    Stateless,
}

impl FlowAssociation {
    pub(crate) fn connection(self) -> Option<(u8, PacketTuple)> {
        match self { Self::Connection { protocol, tuple, .. } => Some((protocol, tuple)), _ => None }
    }
    pub(crate) fn event(self) -> FlowEvent {
        match self { Self::Connection { event, .. } => event, _ => FlowEvent::Related }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RewritePlan {
    Keep,
    Transport(PacketTuple),
    Related { original: PacketTuple, sender: Ipv4Addr, recipient: Ipv4Addr },
}

trait PacketProtocol: Sized {
    fn parse(src: Ipv4Addr, dst: Ipv4Addr, bytes: &[u8]) -> Option<Self>;
    fn association(&self, src: Ipv4Addr, dst: Ipv4Addr) -> FlowAssociation;
    /// Return the new IPv4 addresses; mismatched rewrite plans are rejected.
    fn rewrite(&self, bytes: &mut [u8], plan: RewritePlan) -> Option<(Ipv4Addr, Ipv4Addr)>;
}

#[derive(Clone, Debug)]
pub(super) enum TransportPacket {
    Tcp(tcp::TcpPacket),
    Udp(udp::UdpPacket),
    Icmp(icmp::IcmpPacket),
    Other(u8),
}

impl TransportPacket {
    pub(super) fn parse(protocol: u8, src: Ipv4Addr, dst: Ipv4Addr, bytes: &[u8]) -> Option<Self> {
        Some(match protocol {
            6 => Self::Tcp(tcp::TcpPacket::parse(src, dst, bytes)?),
            17 => Self::Udp(udp::UdpPacket::parse(src, dst, bytes)?),
            1 => Self::Icmp(icmp::IcmpPacket::parse(src, dst, bytes)?),
            other => Self::Other(other),
        })
    }
    pub(super) fn number(&self) -> u8 {
        match self { Self::Tcp(_) => 6, Self::Udp(_) => 17, Self::Icmp(_) => 1, Self::Other(number) => *number }
    }
    pub(super) fn association(&self, src: Ipv4Addr, dst: Ipv4Addr) -> FlowAssociation {
        match self {
            Self::Tcp(packet) => packet.association(src, dst),
            Self::Udp(packet) => packet.association(src, dst),
            Self::Icmp(packet) => packet.association(src, dst),
            Self::Other(_) => FlowAssociation::Stateless,
        }
    }
    pub(super) fn rewrite(&self, bytes: &mut [u8], plan: RewritePlan) -> Option<(Ipv4Addr, Ipv4Addr)> {
        match self {
            Self::Tcp(packet) => packet.rewrite(bytes, plan),
            Self::Udp(packet) => packet.rewrite(bytes, plan),
            Self::Icmp(packet) => packet.rewrite(bytes, plan),
            Self::Other(_) => None,
        }
    }
}

fn rewrite_ports(bytes: &mut [u8], plan: RewritePlan, protocol: u8, checksum_at: usize, optional_checksum: bool) -> Option<(Ipv4Addr, Ipv4Addr)> {
    let RewritePlan::Transport(tuple) = plan else { return None };
    let enabled = !optional_checksum || bytes[checksum_at..checksum_at + 2] != [0, 0];
    bytes[..2].copy_from_slice(&tuple.src_port.to_be_bytes());
    bytes[2..4].copy_from_slice(&tuple.dst_port.to_be_bytes());
    if enabled {
        bytes[checksum_at..checksum_at + 2].fill(0);
        let sum = transport_checksum(tuple.src, tuple.dst, protocol, bytes);
        let sum = if optional_checksum && sum == 0 { u16::MAX } else { sum };
        bytes[checksum_at..checksum_at + 2].copy_from_slice(&sum.to_be_bytes());
    }
    Some((tuple.src, tuple.dst))
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FlowEvent { Tcp(tcp::TcpSegment), Udp, Related }

impl FlowEvent {
    pub(crate) fn starts_new_tcp(self) -> bool { matches!(self, Self::Tcp(segment) if segment.initial_syn()) }
    pub(crate) fn initial_state(self) -> Option<FlowState> {
        match self {
            Self::Tcp(segment) if segment.initial_syn() => Some(FlowState::Tcp(tcp::TcpFlowState::new(segment))),
            Self::Udp => Some(FlowState::Udp(udp::UdpFlowState)),
            _ => None,
        }
    }
}

/// Protocol implementations own lifecycle events and timeout policy.
trait FlowLifecycle: Sized {
    fn deadline(self, last: Instant) -> Instant;
    fn on_delivered(&mut self, event: FlowEvent, from_initiator: bool, now: Instant) -> bool;
    fn is_closing(self) -> bool { false }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FlowState { Tcp(tcp::TcpFlowState), Udp(udp::UdpFlowState) }

impl FlowState {
    pub(crate) fn deadline(self, last: Instant) -> Instant {
        match self { Self::Tcp(state) => state.deadline(last), Self::Udp(state) => state.deadline(last) }
    }
    /// Only successful transport delivery advances state or refreshes idle time.
    pub(crate) fn on_delivered(&mut self, event: FlowEvent, from_initiator: bool, now: Instant) -> bool {
        match self { Self::Tcp(state) => state.on_delivered(event, from_initiator, now), Self::Udp(state) => state.on_delivered(event, from_initiator, now) }
    }
    pub(crate) fn is_closing(self) -> bool {
        match self { Self::Tcp(state) => state.is_closing(), Self::Udp(state) => state.is_closing() }
    }
    #[cfg(test)]
    pub(super) fn tcp_state(self) -> Option<tcp::TcpState> { match self { Self::Tcp(state) => Some(state.handshake), _ => None } }
    #[cfg(test)]
    pub(super) fn fin_directions(self) -> u8 { match self { Self::Tcp(state) => state.fin_directions, _ => 0 } }
    #[cfg(test)]
    pub(super) fn closed_until(self) -> Option<Instant> { match self { Self::Tcp(state) => state.closed_until, _ => None } }
}

#[cfg(test)]
pub(super) use tcp::{TcpState, TCP_HANDSHAKE_IDLE, TCP_ESTABLISHED_IDLE, TCP_CLOSED_GRACE};
#[cfg(test)]
pub(super) use udp::UDP_IDLE;
