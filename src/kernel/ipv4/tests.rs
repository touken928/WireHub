use super::*;
use crate::kernel::{
    checksum::{read_u16, transport_checksum, transport_valid},
    protocol::PacketTuple,
};

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
fn protocol_rewrites_preserve_outer_ipv4_options_and_decrement_ttl_once() {
    let client = Ipv4Addr::new(10, 77, 0, 2);
    let hub = Ipv4Addr::new(10, 77, 0, 1);
    let backend = Ipv4Addr::new(10, 77, 0, 3);
    let translated = PacketTuple {
        src: hub,
        src_port: 40000,
        dst: backend,
        dst_port: 443,
    };
    let udp = test_packet(client.octets(), backend.octets(), false, false);
    let error = test_icmp_error(backend, client, 3, 4, &udp[..28]);
    let related = RewritePlan::Related {
        original: translated,
        sender: hub,
        recipient: client,
    };
    for (raw, plan) in [
        (tcp_packet(b"odd"), RewritePlan::Transport(translated)),
        (udp, RewritePlan::Transport(translated)),
        (error, related),
    ] {
        let expected = parse(&raw).unwrap().decrement_ttl().rewrite(plan).unwrap();
        let mut with_options = raw.clone();
        with_options.splice(20..20, [1, 1, 1, 0]);
        with_options[0] = 0x46;
        let total = with_options.len() as u16;
        with_options[2..4].copy_from_slice(&total.to_be_bytes());
        fix_ip(&mut with_options);
        let actual = parse(&with_options)
            .unwrap()
            .decrement_ttl()
            .rewrite(plan)
            .unwrap();
        assert_eq!(actual[8], 63);
        assert_eq!(checksum(&actual[..24]), 0);
        assert_eq!(&actual[20..24], &[1, 1, 1, 0]);
        assert_eq!(&actual[12..20], &expected[12..20]);
        assert_eq!(
            &actual[24..],
            &expected[20..],
            "protocol rewrite must be independent of IPv4 header length"
        );
    }
}

#[test]
fn incompatible_rewrite_plans_fail_closed() {
    let src = Ipv4Addr::new(10, 77, 0, 2);
    let dst = Ipv4Addr::new(10, 77, 0, 3);
    let tuple = PacketTuple {
        src,
        src_port: 1234,
        dst,
        dst_port: 80,
    };
    let related = RewritePlan::Related {
        original: tuple,
        sender: dst,
        recipient: src,
    };
    let udp = test_packet(src.octets(), dst.octets(), false, false);
    assert!(parse(&udp).unwrap().rewrite(related).is_none());
    assert!(parse(&tcp_packet(b"payload"))
        .unwrap()
        .rewrite(related)
        .is_none());
    let error = test_icmp_error(dst, src, 3, 3, &udp);
    assert!(parse(&error)
        .unwrap()
        .rewrite(RewritePlan::Transport(tuple))
        .is_none());
}

#[test]
fn icmp_nat_rewrite_preserves_truncated_quotes_options_and_transport_checksums() {
    let hub = Ipv4Addr::new(10, 77, 0, 1);
    let backend = Ipv4Addr::new(10, 77, 0, 3);
    let client = Ipv4Addr::new(10, 77, 0, 2);
    for proto in [6, 17] {
        for full in [false, true] {
            let mut quote = if proto == 6 {
                tcp_packet(b"quoted payload")
            } else {
                test_packet(hub.octets(), backend.octets(), false, false)
            };
            quote[12..16].copy_from_slice(&hub.octets());
            quote[16..20].copy_from_slice(&backend.octets());
            quote[20..22].copy_from_slice(&40000u16.to_be_bytes());
            quote[22..24].copy_from_slice(&443u16.to_be_bytes());
            let at = if proto == 6 { 36 } else { 26 };
            quote[at..at + 2].fill(0);
            let sum = transport_checksum(hub, backend, proto, &quote[20..]);
            quote[at..at + 2].copy_from_slice(&sum.to_be_bytes());
            fix_ip(&mut quote);
            // Include IPv4 options, whose checksum must also be repaired.
            quote.splice(20..20, [1, 1, 1, 0]);
            quote[0] = 0x46;
            let len = quote.len() as u16;
            quote[2..4].copy_from_slice(&len.to_be_bytes());
            fix_ip(&mut quote);
            let quoted_len = if full { quote.len() } else { 32 };
            let raw = test_icmp_error(backend, hub, 3, 4, &quote[..quoted_len]);
            let packet = parse(&raw).unwrap().decrement_ttl();
            let result = packet
                .rewrite(RewritePlan::Related {
                    original: PacketTuple {
                        src: client,
                        src_port: 1234,
                        dst: hub,
                        dst_port: 443,
                    },
                    sender: hub,
                    recipient: client,
                })
                .unwrap();
            assert_eq!(checksum(&result[..20]), 0);
            assert_eq!(checksum(&result[20..]), 0);
            assert_eq!(&result[12..16], &hub.octets());
            assert_eq!(&result[16..20], &client.octets());
            assert_eq!(result[8], 63);
            assert_eq!(read_u16(&result, 26), 1280);
            assert_eq!(checksum(&result[28..52]), 0);
            assert_eq!(&result[40..44], &client.octets());
            assert_eq!(&result[44..48], &hub.octets());
            assert_eq!(read_u16(&result, 52), 1234);
            assert_eq!(read_u16(&result, 54), 443);
            if full {
                assert!(transport_valid(client, hub, proto, &result[52..]));
            }
            assert_eq!(result.len(), raw.len());
        }
    }
    let quote = test_packet(hub.octets(), backend.octets(), false, false);
    let packet = parse(&test_icmp_error(backend, hub, 3, 3, &quote)).unwrap();
    let raw = packet
        .rewrite(RewritePlan::Related {
            original: PacketTuple {
                src: client,
                src_port: 1234,
                dst: hub,
                dst_port: 443,
            },
            sender: hub,
            recipient: client,
        })
        .unwrap();
    assert_eq!(
        read_u16(&raw, 54),
        0,
        "IPv4 UDP zero checksum remains disabled"
    );
}

#[test]
fn malformed_icmp_errors_and_quoted_headers_are_rejected() {
    let hub = Ipv4Addr::new(10, 77, 0, 1);
    let backend = Ipv4Addr::new(10, 77, 0, 3);
    let quote = test_packet(hub.octets(), backend.octets(), false, false);
    let valid = test_icmp_error(backend, hub, 3, 3, &quote);
    assert!(parse(&valid).is_some());
    for length in 0..28 {
        assert!(parse(&test_icmp_error(backend, hub, 3, 3, &quote[..length])).is_none());
    }
    let mut corrupt = valid.clone();
    corrupt[30] ^= 1;
    assert!(parse(&corrupt).is_none());
    for (kind, code) in [(3, 16), (11, 2), (12, 3)] {
        assert!(parse(&test_icmp_error(backend, hub, kind, code, &quote)).is_none());
    }
    for mutation in 0..5 {
        let mut bad = quote.clone();
        match mutation {
            0 => bad[0] = 0x44,
            1 => bad[0] = 0x4f,
            2 => bad[6] = 0x20,
            3 => bad[9] = 1,
            _ => bad[2..4].copy_from_slice(&20u16.to_be_bytes()),
        }
        if mutation != 1 {
            fix_ip(&mut bad);
        }
        assert!(parse(&test_icmp_error(backend, hub, 3, 3, &bad)).is_none());
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
fn associations_bind_connection_protocol_to_event_and_related_has_no_event() {
    use crate::kernel::{
        protocol::{ConnectionEvent, FlowAssociation},
        snapshot::TransportProtocol,
    };
    let tcp = parse(&tcp_packet(&[])).unwrap().association();
    let FlowAssociation::Connection(connection) = tcp else {
        panic!("TCP must create a connection association")
    };
    assert_eq!(connection.event.protocol(), TransportProtocol::Tcp);
    assert!(matches!(connection.event, ConnectionEvent::Tcp(_)));

    let udp = parse(&test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false))
        .unwrap()
        .association();
    let FlowAssociation::Connection(connection) = udp else {
        panic!("UDP must create a connection association")
    };
    assert_eq!(connection.event.protocol(), TransportProtocol::Udp);
    assert!(matches!(connection.event, ConnectionEvent::Udp));

    let quoted = test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
    let related = parse(&test_icmp_error(
        Ipv4Addr::new(10, 77, 0, 3),
        Ipv4Addr::new(10, 77, 0, 2),
        3,
        3,
        &quoted,
    ))
    .unwrap()
    .association();
    assert!(matches!(
        related,
        FlowAssociation::Related {
            protocol: TransportProtocol::Udp,
            ..
        }
    ));
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
    let emitted = parse(&packet)
        .unwrap()
        .decrement_ttl()
        .rewrite(RewritePlan::Keep)
        .unwrap();
    assert_eq!(emitted[8], 1);
    assert!(parse(&emitted).is_none());
}
