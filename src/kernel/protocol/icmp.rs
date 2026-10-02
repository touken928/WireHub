use super::*;
use crate::kernel::checksum::{checksum, read_u16, rewrite_transport_checksum};

#[derive(Clone, Copy, Debug)]
pub(in crate::kernel) struct IcmpPacket { error: Option<IcmpError> }
#[derive(Clone, Copy, Debug)]
struct IcmpError { tuple: PacketTuple, protocol: u8, quote_ihl: usize }

impl PacketProtocol for IcmpPacket {
    fn parse(_src: Ipv4Addr, _dst: Ipv4Addr, bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || checksum(bytes) != 0 { return None; }
        let error = if matches!(bytes[0], 3 | 11 | 12) {
            let code = bytes[1];
            if (bytes[0] == 3 && code > 15) || (bytes[0] == 11 && code > 1) || (bytes[0] == 12 && code > 2) { return None; }
            Some(parse_quote(&bytes[8..])?)
        } else { None };
        Some(Self { error })
    }
    fn association(&self, _src: Ipv4Addr, _dst: Ipv4Addr) -> FlowAssociation {
        self.error.map_or(FlowAssociation::Stateless, |error| FlowAssociation::Related { protocol: error.protocol, tuple: error.tuple })
    }
    fn rewrite(&self, bytes: &mut [u8], plan: RewritePlan) -> Option<(Ipv4Addr, Ipv4Addr)> {
        let RewritePlan::Related { original, sender, recipient } = plan else { return None };
        let error = self.error?;
        let quote = 8;
        let transport = quote + error.quote_ihl;
        let checksum_at = transport + if error.protocol == 6 { 16 } else { 6 };
        if checksum_at + 2 <= bytes.len() {
            let old = read_u16(bytes, checksum_at);
            if error.protocol == 6 || old != 0 {
                let updated = rewrite_transport_checksum(old, error.tuple, original);
                let updated = if error.protocol == 17 && updated == 0 { u16::MAX } else { updated };
                bytes[checksum_at..checksum_at + 2].copy_from_slice(&updated.to_be_bytes());
            }
        }
        bytes[quote + 12..quote + 16].copy_from_slice(&original.src.octets());
        bytes[quote + 16..quote + 20].copy_from_slice(&original.dst.octets());
        bytes[transport..transport + 2].copy_from_slice(&original.src_port.to_be_bytes());
        bytes[transport + 2..transport + 4].copy_from_slice(&original.dst_port.to_be_bytes());
        bytes[quote + 10..quote + 12].fill(0);
        let sum = checksum(&bytes[quote..quote + error.quote_ihl]);
        bytes[quote + 10..quote + 12].copy_from_slice(&sum.to_be_bytes());
        bytes[2..4].fill(0);
        let sum = checksum(bytes);
        bytes[2..4].copy_from_slice(&sum.to_be_bytes());
        Some((sender, recipient))
    }
}

fn parse_quote(quote: &[u8]) -> Option<IcmpError> {
    if quote.len() < 28 || quote[0] >> 4 != 4 { return None; }
    let ihl = (quote[0] & 15) as usize * 4;
    if ihl < 20 || quote.len() < ihl + 8 || (read_u16(quote, 2) as usize) < ihl + 8
        || quote[6] & 0xbf != 0 || quote[7] != 0 || checksum(&quote[..ihl]) != 0
        || !matches!(quote[9], 6 | 17) { return None; }
    let total = read_u16(quote, 2) as usize;
    if (quote[9] == 6 && total < ihl + 20) || (quote[9] == 17 && read_u16(quote, ihl + 4) as usize != total - ihl) { return None; }
    Some(IcmpError { tuple: PacketTuple { src: Ipv4Addr::new(quote[12],quote[13],quote[14],quote[15]), dst: Ipv4Addr::new(quote[16],quote[17],quote[18],quote[19]), src_port: read_u16(quote, ihl), dst_port: read_u16(quote, ihl + 2) }, protocol: quote[9], quote_ihl: ihl })
}
