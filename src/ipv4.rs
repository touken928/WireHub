//! Sole strict parser and packet rewrite/checksum implementation for inner IPv4.
use std::net::Ipv4Addr;

use crate::{model::{Group, Peer}, policy};

#[derive(Clone, Debug)]
pub(crate) struct ValidatedPacket {
    bytes: Vec<u8>,
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    src_port: Option<u16>,
    dst_port: Option<u16>,
    ihl: usize,
    tcp_flags: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PacketTuple {
    pub src: Ipv4Addr,
    pub src_port: u16,
    pub dst: Ipv4Addr,
    pub dst_port: u16,
}

impl ValidatedPacket {
    pub(crate) fn src(&self) -> Ipv4Addr { self.src }
    pub(crate) fn dst(&self) -> Ipv4Addr { self.dst }
    pub(crate) fn protocol(&self) -> u8 { self.protocol }
    pub(crate) fn src_port(&self) -> Option<u16> { self.src_port }
    pub(crate) fn dst_port(&self) -> Option<u16> { self.dst_port }
    pub(crate) fn tcp_flags(&self) -> Option<u8> { self.tcp_flags }
    pub(crate) fn bytes(&self) -> &[u8] { &self.bytes }

    /// Emit the validated packet, applying an optional address/port rewrite.
    /// The input TTL has already been decremented exactly once by `validate`.
    pub(crate) fn emit(mut self, rewrite: Option<PacketTuple>) -> Vec<u8> {
        if let Some(tuple) = rewrite {
            self.bytes[12..16].copy_from_slice(&tuple.src.octets());
            self.bytes[16..20].copy_from_slice(&tuple.dst.octets());
            if self.src_port.is_some() && self.dst_port.is_some() {
                self.bytes[self.ihl..self.ihl + 2].copy_from_slice(&tuple.src_port.to_be_bytes());
                self.bytes[self.ihl + 2..self.ihl + 4].copy_from_slice(&tuple.dst_port.to_be_bytes());
                let csum_at = self.ihl + if self.protocol == 6 { 16 } else { 6 };
                let old = u16::from_be_bytes([self.bytes[csum_at], self.bytes[csum_at + 1]]);
                if self.protocol == 6 || old != 0 {
                    self.bytes[csum_at] = 0;
                    self.bytes[csum_at + 1] = 0;
                    let mut c = transport_checksum(tuple.src, tuple.dst, self.protocol, &self.bytes[self.ihl..]);
                    if self.protocol == 17 && c == 0 { c = u16::MAX; }
                    self.bytes[csum_at..csum_at + 2].copy_from_slice(&c.to_be_bytes());
                }
            }
            self.bytes[10] = 0;
            self.bytes[11] = 0;
            let c = checksum(&self.bytes[..self.ihl]);
            self.bytes[10..12].copy_from_slice(&c.to_be_bytes());
        }
        self.bytes
    }
}

pub(crate) fn group_route_allowed(source: Option<&Group>, destination: Option<&Group>) -> bool {
    matches!((source, destination), (Some(s), Some(d)) if policy::allows(s, d))
}

pub(crate) fn validate(packet: &[u8], source: &Peer, source_group: Option<&Group>) -> Option<ValidatedPacket> {
    let parsed = parse(packet)?;
    if !policy::source_is_valid(source, parsed.src) || source_group.is_none() { return None; }
    Some(parsed.decrement_ttl())
}

/// Test helper for packets that need parsing without authenticated ingress policy.
#[cfg(test)]
pub(crate) fn validate_forwarded(packet: &[u8]) -> Option<ValidatedPacket> {
    Some(parse(packet)?.decrement_ttl_without_decrement())
}

#[cfg(test)]
pub(crate) fn validate_and_forward(packet: &[u8], source: &Peer, group: Option<&Group>) -> Option<(Ipv4Addr, Ipv4Addr, Vec<u8>)> {
    let p = validate(packet, source, group)?;
    let src = p.src();
    let dst = p.dst();
    Some((src, dst, p.emit(None)))
}

impl ValidatedPacket {
    fn decrement_ttl(mut self) -> Self { self.bytes[8] -= 1; self.rechecksum_ip(); self }
    #[cfg(test)]
    fn decrement_ttl_without_decrement(self) -> Self { self }
    fn rechecksum_ip(&mut self) {
        self.bytes[10] = 0; self.bytes[11] = 0;
        let c = checksum(&self.bytes[..self.ihl]);
        self.bytes[10..12].copy_from_slice(&c.to_be_bytes());
    }
}

fn parse(packet: &[u8]) -> Option<ValidatedPacket> {
    if packet.len() < 20 || packet.len() > 65535 || packet[0] >> 4 != 4 { return None; }
    let ihl = (packet[0] & 0x0f).checked_mul(4)? as usize;
    if ihl < 20 || ihl > packet.len() { return None; }
    let total = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if total != packet.len() || total < ihl || packet[6] & 0xbf != 0 || packet[7] != 0 || packet[8] <= 1 || checksum(&packet[..ihl]) != 0 { return None; }
    let src = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let dst = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
    let protocol = packet[9];
    let (src_port, dst_port, tcp_flags) = match protocol {
        17 => {
            if total < ihl + 8 { return None; }
            let len = u16::from_be_bytes([packet[ihl + 4], packet[ihl + 5]]) as usize;
            if len < 8 || len != total - ihl { return None; }
            let sum = u16::from_be_bytes([packet[ihl + 6], packet[ihl + 7]]);
            if sum != 0 && !transport_valid(src, dst, protocol, &packet[ihl..]) { return None; }
            (Some(read_u16(packet, ihl)), Some(read_u16(packet, ihl + 2)), None)
        }
        6 => {
            if total < ihl + 20 { return None; }
            let tcp_len = (packet[ihl + 12] >> 4) as usize * 4;
            if tcp_len < 20 || tcp_len > total - ihl { return None; }
            if !transport_valid(src, dst, protocol, &packet[ihl..]) { return None; }
            (Some(read_u16(packet, ihl)), Some(read_u16(packet, ihl + 2)), Some(packet[ihl + 13]))
        }
        _ => (None, None, None),
    };
    Some(ValidatedPacket { bytes: packet.to_vec(), src, dst, protocol, src_port, dst_port, ihl, tcp_flags })
}

fn read_u16(bytes: &[u8], at: usize) -> u16 { u16::from_be_bytes([bytes[at], bytes[at + 1]]) }
pub(crate) fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks_exact(2) { sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32; }
    if bytes.len() % 2 != 0 { sum += (bytes[bytes.len() - 1] as u32) << 8; }
    while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
    !(sum as u16)
}
pub(crate) fn transport_checksum(src: Ipv4Addr, dst: Ipv4Addr, proto: u8, segment: &[u8]) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + segment.len());
    pseudo.extend_from_slice(&src.octets()); pseudo.extend_from_slice(&dst.octets());
    pseudo.extend_from_slice(&[0, proto]); pseudo.extend_from_slice(&(segment.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(segment); checksum(&pseudo)
}
fn transport_valid(src: Ipv4Addr, dst: Ipv4Addr, proto: u8, segment: &[u8]) -> bool { transport_checksum(src, dst, proto, segment) == 0 }

#[cfg(test)]
pub(super) fn test_packet(src: [u8; 4], dst: [u8; 4], malformed: bool, fragment: bool) -> Vec<u8> {
    let mut packet = vec![0; 28]; packet[0] = 0x45; packet[2..4].copy_from_slice(&28u16.to_be_bytes()); packet[8] = 64; packet[9] = 17;
    packet[12..16].copy_from_slice(&src); packet[16..20].copy_from_slice(&dst); packet[20..22].copy_from_slice(&1234u16.to_be_bytes());
    packet[22..24].copy_from_slice(&5678u16.to_be_bytes()); packet[24..26].copy_from_slice(&8u16.to_be_bytes());
    if malformed { packet[0] = 0x44; } if fragment { packet[6] = 0x20; }
    let c = checksum(&packet[..20]); packet[10..12].copy_from_slice(&c.to_be_bytes()); packet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp_packet(payload: &[u8]) -> Vec<u8> {
        let src = Ipv4Addr::new(10, 77, 0, 2);
        let dst = Ipv4Addr::new(10, 77, 0, 3);
        let tcp_len = 24 + payload.len();
        let mut packet = vec![0; 20 + tcp_len];
        packet[0] = 0x45;
        let total_len = packet.len() as u16;
        packet[2..4].copy_from_slice(&total_len.to_be_bytes());
        packet[8] = 64;
        packet[9] = 6;
        packet[12..16].copy_from_slice(&src.octets());
        packet[16..20].copy_from_slice(&dst.octets());
        packet[20..22].copy_from_slice(&1234u16.to_be_bytes());
        packet[22..24].copy_from_slice(&80u16.to_be_bytes());
        packet[32] = 0x60; // 24-byte TCP header, including four option bytes.
        packet[40..44].copy_from_slice(&[1, 1, 1, 0]);
        packet[44..].copy_from_slice(payload);
        let c = transport_checksum(src, dst, 6, &packet[20..]);
        packet[36..38].copy_from_slice(&c.to_be_bytes());
        fix_ip(&mut packet);
        packet
    }

    fn fix_ip(packet: &mut [u8]) {
        packet[10] = 0;
        packet[11] = 0;
        let ihl = (packet[0] & 15) as usize * 4;
        let c = checksum(&packet[..ihl]);
        packet[10..12].copy_from_slice(&c.to_be_bytes());
    }

    #[test]
    fn udp_zero_checksum_and_odd_payload_are_accepted() {
        let mut packet = test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
        packet.push(0xab);
        let length = packet.len() as u16;
        packet[2..4].copy_from_slice(&length.to_be_bytes());
        packet[24..26].copy_from_slice(&9u16.to_be_bytes());
        fix_ip(&mut packet);
        assert!(parse(&packet).is_some());
    }

    #[test]
    fn malformed_udp_checksum_fragments_and_reserved_flag_are_rejected() {
        let mut bad_checksum = test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
        bad_checksum[26] = 1;
        assert!(parse(&bad_checksum).is_none());
        let mut fragment = test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, true);
        fix_ip(&mut fragment);
        assert!(parse(&fragment).is_none());
        let mut reserved = test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
        reserved[6] = 0x80;
        fix_ip(&mut reserved);
        assert!(parse(&reserved).is_none());
    }

    #[test]
    fn tcp_truncation_is_safe_and_options_are_parsed() {
        let mut short = vec![0; 35];
        short[0] = 0x45;
        short[2..4].copy_from_slice(&35u16.to_be_bytes());
        assert!(parse(&short).is_none());
        let packet = tcp_packet(&[]);
        assert_eq!(parse(&packet).unwrap().src_port(), Some(1234));
    }

    #[test]
    fn valid_tcp_checksum_with_options_and_odd_payload_is_accepted() {
        let packet = tcp_packet(&[0x41, 0x42, 0x43]);
        assert!(parse(&packet).is_some());
    }

    #[test]
    fn invalid_tcp_checksum_is_rejected_before_validation() {
        let mut bad_checksum = tcp_packet(&[0x41, 0x42, 0x43]);
        bad_checksum[36] ^= 1;
        assert!(parse(&bad_checksum).is_none());
    }

    #[test]
    fn tcp_payload_bitflip_is_rejected_before_validation() {
        let mut bad_payload = tcp_packet(&[0x41, 0x42, 0x43]);
        *bad_payload.last_mut().unwrap() ^= 1;
        assert!(parse(&bad_payload).is_none());
    }

    #[test]
    fn ttl_decrements_once_and_ttl_one_is_rejected() {
        let mut packet = test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
        packet[8] = 2;
        fix_ip(&mut packet);
        let emitted = parse(&packet).unwrap().decrement_ttl().emit(None);
        assert_eq!(emitted[8], 1);
        assert!(parse(&emitted).is_none());
    }
}
