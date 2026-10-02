//! Strict IPv4 envelope validation; transport behavior is delegated to protocols.
use super::checksum::checksum;
use super::{
    policy,
    protocol::{FlowAssociation, RewritePlan, TransportPacket},
};
use crate::kernel::snapshot::PeerConfigView;
use crate::model::Group;
use std::net::Ipv4Addr;

#[derive(Clone, Debug)]
pub(crate) struct ValidatedPacket {
    bytes: Vec<u8>,
    src: Ipv4Addr,
    dst: Ipv4Addr,
    ihl: usize,
    transport: TransportPacket,
}

impl ValidatedPacket {
    pub(crate) fn src(&self) -> Ipv4Addr {
        self.src
    }
    pub(crate) fn dst(&self) -> Ipv4Addr {
        self.dst
    }
    pub(crate) fn protocol(&self) -> u8 {
        self.transport.number()
    }
    pub(crate) fn association(&self) -> FlowAssociation {
        self.transport.association(self.src, self.dst)
    }
    #[cfg(test)]
    pub(crate) fn src_port(&self) -> Option<u16> {
        self.association()
            .connection()
            .map(|connection| connection.tuple.src_port)
    }
    pub(crate) fn dst_port(&self) -> Option<u16> {
        self.association()
            .connection()
            .map(|connection| connection.tuple.dst_port)
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Rewrites protocol contents, then updates the IPv4 envelope checksum.
    /// TTL is decremented only during authenticated ingress validation.
    pub(crate) fn rewrite(mut self, plan: RewritePlan) -> Option<Vec<u8>> {
        if !matches!(plan, RewritePlan::Keep) {
            let (src, dst) = self.transport.rewrite(&mut self.bytes[self.ihl..], plan)?;
            self.bytes[12..16].copy_from_slice(&src.octets());
            self.bytes[16..20].copy_from_slice(&dst.octets());
            self.rechecksum_ip();
        }
        Some(self.bytes)
    }
    fn decrement_ttl(mut self) -> Self {
        self.bytes[8] -= 1;
        self.rechecksum_ip();
        self
    }
    fn rechecksum_ip(&mut self) {
        self.bytes[10..12].fill(0);
        let c = checksum(&self.bytes[..self.ihl]);
        self.bytes[10..12].copy_from_slice(&c.to_be_bytes());
    }
}

pub(crate) fn validate(
    packet: &[u8],
    source: &impl PeerConfigView,
    source_group: Option<&Group>,
) -> Option<ValidatedPacket> {
    let parsed = parse(packet)?;
    if !policy::source_is_valid(source, parsed.src) || source_group.is_none() {
        return None;
    }
    Some(parsed.decrement_ttl())
}

/// Test helper for packets that need parsing without authenticated ingress policy.
#[cfg(test)]
pub(crate) fn validate_forwarded(packet: &[u8]) -> Option<ValidatedPacket> {
    parse(packet)
}

#[cfg(test)]
pub(crate) fn validate_and_forward(
    packet: &[u8],
    source: &impl PeerConfigView,
    group: Option<&Group>,
) -> Option<(Ipv4Addr, Ipv4Addr, Vec<u8>)> {
    let p = validate(packet, source, group)?;
    let src = p.src();
    let dst = p.dst();
    Some((src, dst, p.rewrite(RewritePlan::Keep)?))
}

fn parse(packet: &[u8]) -> Option<ValidatedPacket> {
    if packet.len() < 20 || packet.len() > 65535 || packet[0] >> 4 != 4 {
        return None;
    }
    let ihl = (packet[0] & 0x0f).checked_mul(4)? as usize;
    if ihl < 20 || ihl > packet.len() {
        return None;
    }
    let total = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if total != packet.len()
        || total < ihl
        || packet[6] & 0xbf != 0
        || packet[7] != 0
        || packet[8] <= 1
        || checksum(&packet[..ihl]) != 0
    {
        return None;
    }
    let src = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let dst = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
    let transport = TransportPacket::parse(packet[9], src, dst, &packet[ihl..])?;
    Some(ValidatedPacket {
        bytes: packet.to_vec(),
        src,
        dst,
        ihl,
        transport,
    })
}

#[cfg(test)]
pub(super) fn test_packet(src: [u8; 4], dst: [u8; 4], malformed: bool, fragment: bool) -> Vec<u8> {
    let mut packet = vec![0; 28];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&28u16.to_be_bytes());
    packet[8] = 64;
    packet[9] = 17;
    packet[12..16].copy_from_slice(&src);
    packet[16..20].copy_from_slice(&dst);
    packet[20..22].copy_from_slice(&1234u16.to_be_bytes());
    packet[22..24].copy_from_slice(&5678u16.to_be_bytes());
    packet[24..26].copy_from_slice(&8u16.to_be_bytes());
    if malformed {
        packet[0] = 0x44;
    }
    if fragment {
        packet[6] = 0x20;
    }
    let c = checksum(&packet[..20]);
    packet[10..12].copy_from_slice(&c.to_be_bytes());
    packet
}

#[cfg(test)]
pub(crate) fn test_icmp_error(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    kind: u8,
    code: u8,
    quote: &[u8],
) -> Vec<u8> {
    let mut raw = vec![0; 28 + quote.len()];
    raw[0] = 0x45;
    let len = raw.len() as u16;
    raw[2..4].copy_from_slice(&len.to_be_bytes());
    raw[8] = 64;
    raw[9] = 1;
    raw[12..16].copy_from_slice(&src.octets());
    raw[16..20].copy_from_slice(&dst.octets());
    raw[20] = kind;
    raw[21] = code;
    raw[28..].copy_from_slice(quote);
    if kind == 3 && code == 4 {
        raw[26..28].copy_from_slice(&1280u16.to_be_bytes());
    }
    let sum = checksum(&raw[20..]);
    raw[22..24].copy_from_slice(&sum.to_be_bytes());
    let sum = checksum(&raw[..20]);
    raw[10..12].copy_from_slice(&sum.to_be_bytes());
    raw
}

#[cfg(test)]
mod tests;
