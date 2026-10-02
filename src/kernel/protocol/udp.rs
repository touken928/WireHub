use std::time::Duration;
use super::*;
use crate::kernel::checksum::{read_u16, transport_valid};

pub(in crate::kernel) const UDP_IDLE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
pub(in crate::kernel) struct UdpPacket { src_port: u16, dst_port: u16 }

impl PacketProtocol for UdpPacket {
    fn parse(src: Ipv4Addr, dst: Ipv4Addr, bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || read_u16(bytes, 4) as usize != bytes.len() { return None; }
        if read_u16(bytes, 6) != 0 && !transport_valid(src, dst, 17, bytes) { return None; }
        Some(Self { src_port: read_u16(bytes, 0), dst_port: read_u16(bytes, 2) })
    }
    fn association(&self, src: Ipv4Addr, dst: Ipv4Addr) -> FlowAssociation {
        FlowAssociation::Connection { protocol: 17, tuple: PacketTuple { src, dst, src_port: self.src_port, dst_port: self.dst_port }, event: FlowEvent::Udp }
    }
    fn rewrite(&self, bytes: &mut [u8], plan: RewritePlan) -> Option<(Ipv4Addr, Ipv4Addr)> { rewrite_ports(bytes, plan, 17, 6, true) }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpFlowState;

impl FlowLifecycle for UdpFlowState {
    fn deadline(self, last: Instant) -> Instant { last + UDP_IDLE }
    fn on_delivered(&mut self, event: FlowEvent, _from_initiator: bool, _now: Instant) -> bool { matches!(event, FlowEvent::Udp) }
}
