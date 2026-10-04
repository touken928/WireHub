use super::protocol::PacketTuple;
use std::net::Ipv4Addr;

pub(super) fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}
pub(crate) fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks_exact(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if !bytes.len().is_multiple_of(2) {
        sum += (bytes[bytes.len() - 1] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
pub(crate) fn transport_checksum(src: Ipv4Addr, dst: Ipv4Addr, proto: u8, segment: &[u8]) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + segment.len());
    pseudo.extend_from_slice(&src.octets());
    pseudo.extend_from_slice(&dst.octets());
    pseudo.extend_from_slice(&[0, proto]);
    pseudo.extend_from_slice(&(segment.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(segment);
    checksum(&pseudo)
}
pub(super) fn transport_valid(src: Ipv4Addr, dst: Ipv4Addr, proto: u8, segment: &[u8]) -> bool {
    transport_checksum(src, dst, proto, segment) == 0
}

pub(super) fn rewrite_transport_checksum(old: u16, before: PacketTuple, after: PacketTuple) -> u16 {
    let words = |tuple: PacketTuple| {
        let s = tuple.src.octets();
        let d = tuple.dst.octets();
        [
            u16::from_be_bytes([s[0], s[1]]),
            u16::from_be_bytes([s[2], s[3]]),
            u16::from_be_bytes([d[0], d[1]]),
            u16::from_be_bytes([d[2], d[3]]),
            tuple.src_port,
            tuple.dst_port,
        ]
    };
    let mut sum = (!old) as u32;
    for (old, new) in words(before).into_iter().zip(words(after)) {
        sum += (!old) as u32 + new as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
