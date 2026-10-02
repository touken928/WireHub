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
    tcp: Option<TcpSegment>,
    icmp_error: Option<IcmpError>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TcpSegment { pub flags: u8, pub seq: u32, pub ack: u32, pub payload_len: u32 }

#[derive(Clone, Copy, Debug)]
pub(crate) struct IcmpError { pub tuple: PacketTuple, pub protocol: u8, quote_ihl: usize }

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
    pub(crate) fn tcp_segment(&self) -> Option<TcpSegment> { self.tcp }
    pub(crate) fn icmp_error(&self) -> Option<IcmpError> { self.icmp_error }
    pub(crate) fn is_icmp_error(&self) -> bool { self.protocol == 1 && matches!(self.bytes[self.ihl], 3 | 11 | 12) }
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

    /// ICMP quotes may contain only eight transport bytes. Adjust any quoted
    /// transport checksum incrementally instead of checksumming a truncated segment.
    pub(crate) fn emit_icmp_error(mut self, original: PacketTuple, sender: Ipv4Addr, recipient: Ipv4Addr) -> Vec<u8> {
        let error = self.icmp_error.expect("validated ICMP error");
        let quote = self.ihl + 8;
        let transport = quote + error.quote_ihl;
        let checksum_at = transport + if error.protocol == 6 { 16 } else { 6 };
        if checksum_at + 2 <= self.bytes.len() {
            let old = read_u16(&self.bytes, checksum_at);
            if error.protocol == 6 || old != 0 {
                let updated = rewrite_transport_checksum(old, error.tuple, original);
                let updated = if error.protocol == 17 && updated == 0 { u16::MAX } else { updated };
                self.bytes[checksum_at..checksum_at + 2].copy_from_slice(&updated.to_be_bytes());
            }
        }
        self.bytes[quote + 12..quote + 16].copy_from_slice(&original.src.octets());
        self.bytes[quote + 16..quote + 20].copy_from_slice(&original.dst.octets());
        self.bytes[transport..transport + 2].copy_from_slice(&original.src_port.to_be_bytes());
        self.bytes[transport + 2..transport + 4].copy_from_slice(&original.dst_port.to_be_bytes());
        self.bytes[quote + 10..quote + 12].fill(0);
        let sum = checksum(&self.bytes[quote..quote + error.quote_ihl]);
        self.bytes[quote + 10..quote + 12].copy_from_slice(&sum.to_be_bytes());
        self.bytes[self.ihl + 2..self.ihl + 4].fill(0);
        let sum = checksum(&self.bytes[self.ihl..]);
        self.bytes[self.ihl + 2..self.ihl + 4].copy_from_slice(&sum.to_be_bytes());
        self.bytes[12..16].copy_from_slice(&sender.octets());
        self.bytes[16..20].copy_from_slice(&recipient.octets());
        self.rechecksum_ip();
        self.bytes
    }
}

fn rewrite_transport_checksum(old: u16, before: PacketTuple, after: PacketTuple) -> u16 {
    let words = |tuple: PacketTuple| {
        let s = tuple.src.octets(); let d = tuple.dst.octets();
        [u16::from_be_bytes([s[0],s[1]]),u16::from_be_bytes([s[2],s[3]]),
         u16::from_be_bytes([d[0],d[1]]),u16::from_be_bytes([d[2],d[3]]),tuple.src_port,tuple.dst_port]
    };
    let mut sum = (!old) as u32;
    for (old, new) in words(before).into_iter().zip(words(after)) { sum += (!old) as u32 + new as u32; }
    while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
    !(sum as u16)
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
    let mut tcp = None;
    let mut icmp_error = None;
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
            tcp = Some(TcpSegment { flags: packet[ihl + 13], seq: u32::from_be_bytes(packet[ihl + 4..ihl + 8].try_into().ok()?), ack: u32::from_be_bytes(packet[ihl + 8..ihl + 12].try_into().ok()?), payload_len: (total - ihl - tcp_len) as u32 });
            (Some(read_u16(packet, ihl)), Some(read_u16(packet, ihl + 2)), Some(packet[ihl + 13]))
        }
        1 => {
            if total < ihl + 8 || checksum(&packet[ihl..]) != 0 { return None; }
            if matches!(packet[ihl], 3 | 11 | 12) {
                let code = packet[ihl + 1];
                if (packet[ihl] == 3 && code > 15) || (packet[ihl] == 11 && code > 1) || (packet[ihl] == 12 && code > 2) { return None; }
                icmp_error = Some(parse_icmp_quote(&packet[ihl + 8..])?);
            }
            (None, None, None)
        }
        _ => (None, None, None),
    };
    Some(ValidatedPacket { bytes: packet.to_vec(), src, dst, protocol, src_port, dst_port, ihl, tcp_flags, tcp, icmp_error })
}

fn parse_icmp_quote(quote: &[u8]) -> Option<IcmpError> {
    if quote.len() < 28 || quote[0] >> 4 != 4 { return None; }
    let ihl = (quote[0] & 15) as usize * 4;
    if ihl < 20 || quote.len() < ihl + 8 || (read_u16(quote, 2) as usize) < ihl + 8
        || quote[6] & 0xbf != 0 || quote[7] != 0 || checksum(&quote[..ihl]) != 0
        || !matches!(quote[9], 6 | 17) { return None; }
    let total = read_u16(quote, 2) as usize;
    if (quote[9] == 6 && total < ihl + 20) || (quote[9] == 17 && read_u16(quote, ihl + 4) as usize != total - ihl) { return None; }
    Some(IcmpError { tuple: PacketTuple { src: Ipv4Addr::new(quote[12],quote[13],quote[14],quote[15]), dst: Ipv4Addr::new(quote[16],quote[17],quote[18],quote[19]), src_port: read_u16(quote, ihl), dst_port: read_u16(quote, ihl + 2) }, protocol: quote[9], quote_ihl: ihl })
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
pub(crate) fn test_icmp_error(src: Ipv4Addr, dst: Ipv4Addr, kind: u8, code: u8, quote: &[u8]) -> Vec<u8> {
    let mut raw = vec![0; 28 + quote.len()]; raw[0] = 0x45;
    let len = raw.len() as u16; raw[2..4].copy_from_slice(&len.to_be_bytes()); raw[8] = 64; raw[9] = 1;
    raw[12..16].copy_from_slice(&src.octets()); raw[16..20].copy_from_slice(&dst.octets());
    raw[20] = kind; raw[21] = code; raw[28..].copy_from_slice(quote);
    if kind == 3 && code == 4 { raw[26..28].copy_from_slice(&1280u16.to_be_bytes()); }
    let sum = checksum(&raw[20..]); raw[22..24].copy_from_slice(&sum.to_be_bytes());
    let sum = checksum(&raw[..20]); raw[10..12].copy_from_slice(&sum.to_be_bytes()); raw
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
    fn icmp_nat_rewrite_preserves_truncated_quotes_options_and_transport_checksums() {
        let hub = Ipv4Addr::new(10,77,0,1); let backend = Ipv4Addr::new(10,77,0,3); let client = Ipv4Addr::new(10,77,0,2);
        for proto in [6, 17] {
            for full in [false, true] {
                let mut quote = if proto == 6 { tcp_packet(b"quoted payload") } else { test_packet(hub.octets(),backend.octets(),false,false) };
                quote[12..16].copy_from_slice(&hub.octets()); quote[16..20].copy_from_slice(&backend.octets());
                quote[20..22].copy_from_slice(&40000u16.to_be_bytes()); quote[22..24].copy_from_slice(&443u16.to_be_bytes());
                let at = if proto == 6 { 36 } else { 26 }; quote[at..at+2].fill(0);
                let sum = transport_checksum(hub,backend,proto,&quote[20..]); quote[at..at+2].copy_from_slice(&sum.to_be_bytes()); fix_ip(&mut quote);
                // Include IPv4 options, whose checksum must also be repaired.
                quote.splice(20..20, [1,1,1,0]); quote[0] = 0x46;
                let len = quote.len() as u16; quote[2..4].copy_from_slice(&len.to_be_bytes()); fix_ip(&mut quote);
                let quoted_len = if full { quote.len() } else { 32 };
                let raw = test_icmp_error(backend,hub,3,4,&quote[..quoted_len]);
                let packet = parse(&raw).unwrap().decrement_ttl();
                let result = packet.emit_icmp_error(PacketTuple { src:client,src_port:1234,dst:hub,dst_port:443 },hub,client);
                assert_eq!(checksum(&result[..20]),0); assert_eq!(checksum(&result[20..]),0);
                assert_eq!(&result[12..16],&hub.octets()); assert_eq!(&result[16..20],&client.octets()); assert_eq!(result[8],63);
                assert_eq!(read_u16(&result,26),1280);
                assert_eq!(checksum(&result[28..52]),0);
                assert_eq!(&result[40..44],&client.octets()); assert_eq!(&result[44..48],&hub.octets());
                assert_eq!(read_u16(&result,52),1234); assert_eq!(read_u16(&result,54),443);
                if full { assert!(transport_valid(client,hub,proto,&result[52..])); }
                assert_eq!(result.len(),raw.len());
            }
        }
        let quote = test_packet(hub.octets(),backend.octets(),false,false);
        let packet = parse(&test_icmp_error(backend,hub,3,3,&quote)).unwrap();
        let raw = packet.emit_icmp_error(PacketTuple{src:client,src_port:1234,dst:hub,dst_port:443},hub,client);
        assert_eq!(read_u16(&raw,54),0,"IPv4 UDP zero checksum remains disabled");
    }

    #[test]
    fn malformed_icmp_errors_and_quoted_headers_are_rejected() {
        let hub=Ipv4Addr::new(10,77,0,1); let backend=Ipv4Addr::new(10,77,0,3);
        let quote=test_packet(hub.octets(),backend.octets(),false,false);
        let valid=test_icmp_error(backend,hub,3,3,&quote); assert!(parse(&valid).is_some());
        for length in 0..28 { assert!(parse(&test_icmp_error(backend,hub,3,3,&quote[..length])).is_none()); }
        let mut corrupt=valid.clone(); corrupt[30]^=1; assert!(parse(&corrupt).is_none());
        for (kind,code) in [(3,16),(11,2),(12,3)] { assert!(parse(&test_icmp_error(backend,hub,kind,code,&quote)).is_none()); }
        for mutation in 0..5 {
            let mut bad=quote.clone();
            match mutation { 0=>bad[0]=0x44,1=>bad[0]=0x4f,2=>bad[6]=0x20,3=>bad[9]=1,_=>bad[2..4].copy_from_slice(&20u16.to_be_bytes()) }
            if mutation != 1 { fix_ip(&mut bad); }
            assert!(parse(&test_icmp_error(backend,hub,3,3,&bad)).is_none());
        }
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
