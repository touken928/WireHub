use super::*;
use crate::kernel::checksum::checksum;
use crate::model::Forward;
use crate::model::Peer;

fn peer(id: &str, ip: &str) -> Peer {
    Peer {
        id: id.into(),
        name: id.into(),
        public_key: String::new(),
        ipv4: ip.into(),
        group_id: String::new(),
        received_bytes: 0,
        sent_bytes: 0,
        last_handshake_unix: None,
    }
}
fn forward(proto: &str, port: u16) -> Forward {
    Forward {
        id: "f".into(),
        name: "service".into(),
        protocol: proto.into(),
        target_peer_id: "backend".into(),
        target_port: port,
        allowed_group_ids: vec![],
    }
}
fn packet(
    proto: u8,
    src: Ipv4Addr,
    sport: u16,
    dst: Ipv4Addr,
    dport: u16,
    flags: u8,
) -> ValidatedPacket {
    let transport_len = if proto == 6 { 20 } else { 8 };
    let mut bytes = vec![0; 20 + transport_len];
    bytes[0] = 0x45;
    let total_len = bytes.len() as u16;
    bytes[2..4].copy_from_slice(&total_len.to_be_bytes());
    bytes[8] = 64;
    bytes[9] = proto;
    bytes[12..16].copy_from_slice(&src.octets());
    bytes[16..20].copy_from_slice(&dst.octets());
    bytes[20..22].copy_from_slice(&sport.to_be_bytes());
    bytes[22..24].copy_from_slice(&dport.to_be_bytes());
    if proto == 6 {
        bytes[32] = 0x50;
        bytes[33] = flags;
    } else {
        bytes[24..26].copy_from_slice(&8u16.to_be_bytes());
    }
    if proto == 6 {
        let c = tcp_checksum(src.octets(), dst.octets(), &bytes[20..]);
        bytes[36..38].copy_from_slice(&c.to_be_bytes());
    }
    let checksum = checksum(&bytes[..20]);
    bytes[10..12].copy_from_slice(&checksum.to_be_bytes());
    ipv4::validate_forwarded(&bytes).unwrap()
}
fn tcp_checksum(src: [u8; 4], dst: [u8; 4], segment: &[u8]) -> u16 {
    let mut pseudo = Vec::new();
    pseudo.extend_from_slice(&src);
    pseudo.extend_from_slice(&dst);
    pseudo.extend_from_slice(&[0, 6]);
    pseudo.extend_from_slice(&(segment.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(segment);
    let mut sum = 0u32;
    for c in pseudo.chunks_exact(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if pseudo.len() % 2 != 0 {
        sum += (pseudo[pseudo.len() - 1] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
fn t() -> Instant {
    Instant::now()
}

fn tcp_numbers(packet: ValidatedPacket, seq: u32, ack: u32) -> ValidatedPacket {
    let mut raw = packet.bytes().to_vec();
    raw[24..28].copy_from_slice(&seq.to_be_bytes());
    raw[28..32].copy_from_slice(&ack.to_be_bytes());
    raw[36..38].fill(0);
    let sum = tcp_checksum(packet.src().octets(), packet.dst().octets(), &raw[20..]);
    raw[36..38].copy_from_slice(&sum.to_be_bytes());
    ipv4::validate_forwarded(&raw).unwrap()
}

fn tcp_payload(packet: ValidatedPacket, len: usize) -> ValidatedPacket {
    let mut raw = packet.bytes().to_vec();
    raw.resize(raw.len() + len, 0x5a);
    let total_len = raw.len() as u16;
    raw[2..4].copy_from_slice(&total_len.to_be_bytes());
    raw[10..12].fill(0);
    let ip_sum = checksum(&raw[..20]);
    raw[10..12].copy_from_slice(&ip_sum.to_be_bytes());
    raw[36..38].fill(0);
    let tcp_sum = tcp_checksum(packet.src().octets(), packet.dst().octets(), &raw[20..]);
    raw[36..38].copy_from_slice(&tcp_sum.to_be_bytes());
    ipv4::validate_forwarded(&raw).unwrap()
}

#[test]
fn tcp_handshake_completion_and_idle_boundaries_for_direct_and_forward_flows() {
    let a = peer("a", "10.77.0.2");
    let b = peer("b", "10.77.0.3");
    let now = t();
    for forwarded in [false, true] {
        let mut flows = Flows::default();
        let f = forward("tcp", 443);
        let frontend = if forwarded {
            flows.hub_ip
        } else {
            b.ipv4.parse().unwrap()
        };
        let syn = tcp_numbers(
            packet(6, a.ipv4.parse().unwrap(), 1234, frontend, 443, 2),
            u32::MAX,
            0,
        );
        let (wire, r) = if forwarded {
            flows.prepare_forward_packet_test(&syn, &a, &f, &b, now)
        } else {
            flows.prepare_direct_test(&syn, &a, &b, now)
        }
        .unwrap();
        flows.complete(r, true, now);
        let return_ip = if forwarded {
            flows.hub_ip
        } else {
            a.ipv4.parse().unwrap()
        };
        let return_port = u16::from_be_bytes([wire[20], wire[21]]);
        let syn_ack = tcp_numbers(
            packet(
                6,
                b.ipv4.parse().unwrap(),
                443,
                return_ip,
                return_port,
                0x12,
            ),
            200,
            0,
        );
        let ack = tcp_numbers(
            packet(6, a.ipv4.parse().unwrap(), 1234, frontend, 443, 0x10),
            0,
            201,
        );
        let prepare = |flows: &mut Flows, packet: &ValidatedPacket, at| {
            if forwarded {
                flows.prepare_forward_packet_test(packet, &a, &f, &b, at)
            } else {
                flows.prepare_direct_test(packet, &a, &b, at)
            }
        };
        // A stray ACK before SYN/ACK cannot establish the connection.
        let (_, r) = prepare(&mut flows, &ack, now).unwrap();
        flows.complete(r, true, now);
        let key = flows.flows.keys().next().unwrap().clone();
        assert!(matches!(
            flows.flows[&key].state.tcp_state(),
            Some(TcpState::SynSent { .. })
        ));
        let (_, _, r) = flows.lookup_reply_test(&syn_ack, &b, now).unwrap();
        flows.complete(r.expect("connection reply reservation"), false, now);
        assert!(matches!(
            flows.flows[&key].state.tcp_state(),
            Some(TcpState::SynSent { .. })
        ));
        let (_, _, r) = flows.lookup_reply_test(&syn_ack, &b, now).unwrap();
        flows.complete(r.expect("connection reply reservation"), true, now);
        assert!(matches!(
            flows.flows[&key].state.tcp_state(),
            Some(TcpState::SynReceived { .. })
        ));
        assert_eq!(flows.flows[&key].deadline(), now + TCP_HANDSHAKE_IDLE);
        let wrong_ack = tcp_numbers(ack.clone(), 0, 202);
        let (_, r) = prepare(&mut flows, &wrong_ack, now).unwrap();
        flows.complete(r, true, now);
        assert!(matches!(
            flows.flows[&key].state.tcp_state(),
            Some(TcpState::SynReceived { .. })
        ));
        let (_, r) = prepare(&mut flows, &ack, now).unwrap();
        flows.complete(r, false, now);
        assert!(matches!(
            flows.flows[&key].state.tcp_state(),
            Some(TcpState::SynReceived { .. })
        ));
        let (_, r) = prepare(&mut flows, &ack, now).unwrap();
        flows.complete(r, true, now);
        assert_eq!(
            flows.flows[&key].state.tcp_state(),
            Some(TcpState::Established)
        );
        assert!(prepare(&mut flows, &ack, now + Duration::from_secs(301)).is_some());
        assert!(prepare(
            &mut flows,
            &ack,
            now + TCP_ESTABLISHED_IDLE - Duration::from_secs(1)
        )
        .is_some());
        assert!(prepare(&mut flows, &ack, now + TCP_ESTABLISHED_IDLE).is_none());
        assert!(flows.reverse.is_empty());
        assert!(flows.peer_counts.is_empty());
    }
}

#[test]
fn tcp_tfo_responder_data_extends_syn_received_window_for_direct_and_forward() {
    let a = peer("a", "10.77.0.2");
    let b = peer("b", "10.77.0.3");
    let now = t();
    for forwarded in [false, true] {
        for responder_next in [201u32, u32::MAX - 49] {
            let mut flows = Flows::default();
            let f = forward("tcp", 443);
            let frontend = if forwarded {
                flows.hub_ip
            } else {
                b.ipv4.parse().unwrap()
            };
            let syn = tcp_numbers(
                tcp_payload(
                    packet(6, a.ipv4.parse().unwrap(), 1234, frontend, 443, 2),
                    9,
                ),
                100,
                0,
            );
            let (wire, reservation) = if forwarded {
                flows.prepare_forward_packet_test(&syn, &a, &f, &b, now)
            } else {
                flows.prepare_direct_test(&syn, &a, &b, now)
            }
            .unwrap();
            flows.complete(reservation, true, now);
            let return_ip = if forwarded {
                flows.hub_ip
            } else {
                a.ipv4.parse().unwrap()
            };
            let return_port = u16::from_be_bytes([wire[20], wire[21]]);
            let syn_ack = tcp_numbers(
                packet(
                    6,
                    b.ipv4.parse().unwrap(),
                    443,
                    return_ip,
                    return_port,
                    0x12,
                ),
                responder_next.wrapping_sub(1),
                110,
            );
            let (_, _, reservation) = flows.lookup_reply_test(&syn_ack, &b, now).unwrap();
            flows.complete(reservation.unwrap(), true, now);

            let responder_data = tcp_numbers(
                tcp_payload(
                    packet(
                        6,
                        b.ipv4.parse().unwrap(),
                        443,
                        return_ip,
                        return_port,
                        0x18,
                    ),
                    100,
                ),
                responder_next,
                110,
            );
            let key = flows.flows.keys().next().unwrap().clone();
            let responder_end = responder_next.wrapping_add(100);
            let final_ack = tcp_numbers(
                packet(6, a.ipv4.parse().unwrap(), 1234, frontend, 443, 0x10),
                110,
                responder_end,
            );
            let reservation = if forwarded {
                flows.prepare_forward_packet_test(&final_ack, &a, &f, &b, now)
            } else {
                flows.prepare_direct_test(&final_ack, &a, &b, now)
            }
            .unwrap()
            .1;
            flows.complete(reservation, true, now);
            assert!(
                matches!(flows.flows[&key].state.tcp_state(), Some(TcpState::SynReceived { responder_end: end, .. }) if end == responder_next)
            );

            let (_, _, reservation) = flows.lookup_reply_test(&responder_data, &b, now).unwrap();
            flows.complete(reservation.unwrap(), false, now);
            let reservation = if forwarded {
                flows.prepare_forward_packet_test(&final_ack, &a, &f, &b, now)
            } else {
                flows.prepare_direct_test(&final_ack, &a, &b, now)
            }
            .unwrap()
            .1;
            flows.complete(reservation, true, now);
            assert!(
                matches!(flows.flows[&key].state.tcp_state(), Some(TcpState::SynReceived { responder_end: end, .. }) if end == responder_next)
            );

            let (_, _, reservation) = flows.lookup_reply_test(&responder_data, &b, now).unwrap();
            flows.complete(reservation.unwrap(), true, now);
            assert!(
                matches!(flows.flows[&key].state.tcp_state(), Some(TcpState::SynReceived { responder_next: base, responder_end: end, .. }) if base == responder_next && end == responder_end)
            );

            let oversized_ack = tcp_numbers(final_ack.clone(), 110, responder_end.wrapping_add(1));
            let reservation = if forwarded {
                flows.prepare_forward_packet_test(&oversized_ack, &a, &f, &b, now)
            } else {
                flows.prepare_direct_test(&oversized_ack, &a, &b, now)
            }
            .unwrap()
            .1;
            flows.complete(reservation, true, now);
            assert!(matches!(
                flows.flows[&key].state.tcp_state(),
                Some(TcpState::SynReceived { .. })
            ));

            let reservation = if forwarded {
                flows.prepare_forward_packet_test(&final_ack, &a, &f, &b, now)
            } else {
                flows.prepare_direct_test(&final_ack, &a, &b, now)
            }
            .unwrap()
            .1;
            flows.complete(reservation, true, now);
            assert_eq!(
                flows.flows[&key].state.tcp_state(),
                Some(TcpState::Established)
            );
            flows.expire(now + Duration::from_secs(301));
            assert!(flows.flows.contains_key(&key));
        }
    }
}

#[test]
fn incomplete_tcp_expires_at_sixty_seconds_and_successful_activity_refreshes_it() {
    let a = peer("a", "10.77.0.2");
    let b = peer("b", "10.77.0.3");
    let now = t();
    for syn_ack_seen in [false, true] {
        let mut flows = Flows::default();
        let syn = packet(
            6,
            a.ipv4.parse().unwrap(),
            1234,
            b.ipv4.parse().unwrap(),
            443,
            2,
        );
        let (_, r) = flows.prepare_direct_test(&syn, &a, &b, now).unwrap();
        flows.complete(r, true, now);
        if syn_ack_seen {
            let syn_ack = tcp_numbers(
                packet(
                    6,
                    b.ipv4.parse().unwrap(),
                    443,
                    a.ipv4.parse().unwrap(),
                    1234,
                    0x12,
                ),
                200,
                1,
            );
            let (_, _, r) = flows.lookup_reply_test(&syn_ack, &b, now).unwrap();
            flows.complete(r.expect("connection reply reservation"), true, now);
        }
        flows.expire(now + TCP_HANDSHAKE_IDLE - Duration::from_secs(1));
        assert_eq!(flows.flows.len(), 1);
        flows.expire(now + TCP_HANDSHAKE_IDLE);
        assert!(flows.flows.is_empty());
        assert!(flows.peer_counts.is_empty());
    }
    let mut flows = Flows::default();
    let syn = packet(
        6,
        a.ipv4.parse().unwrap(),
        1234,
        b.ipv4.parse().unwrap(),
        443,
        2,
    );
    let (_, r) = flows.prepare_direct_test(&syn, &a, &b, now).unwrap();
    flows.complete(r, true, now);
    let (_, r) = flows
        .prepare_direct_test(&syn, &a, &b, now + Duration::from_secs(59))
        .unwrap();
    flows.complete(r, true, now + Duration::from_secs(59));
    flows.expire(now + Duration::from_secs(60));
    assert_eq!(flows.flows.len(), 1);
    flows.expire(now + Duration::from_secs(119));
    assert!(flows.flows.is_empty());
}

#[test]
fn related_icmp_requires_committed_live_flow_and_matching_backend_identity() {
    let a = peer("a", "10.77.0.2");
    let b = peer("b", "10.77.0.3");
    let now = t();
    for proto in [6, 17] {
        for forwarded in [false, true] {
            let mut flows = Flows::default();
            let f = forward(if proto == 6 { "tcp" } else { "udp" }, 443);
            let frontend = if forwarded {
                flows.hub_ip
            } else {
                b.ipv4.parse().unwrap()
            };
            let original = packet(proto, a.ipv4.parse().unwrap(), 1234, frontend, 443, 2);
            let (wire, r) = if forwarded {
                flows.prepare_forward_packet_test(&original, &a, &f, &b, now)
            } else {
                flows.prepare_direct_test(&original, &a, &b, now)
            }
            .unwrap();
            let translated_src = Ipv4Addr::new(wire[12], wire[13], wire[14], wire[15]);
            for (kind, code) in [(3, 3), (3, 4), (11, 0), (12, 0)] {
                let raw = ipv4::test_icmp_error(
                    b.ipv4.parse().unwrap(),
                    translated_src,
                    kind,
                    code,
                    &wire[..28],
                );
                let error = ipv4::validate_forwarded(&raw).unwrap();
                assert!(
                    flows.lookup_reply_test(&error, &b, now).is_none(),
                    "pending reservations grant no error access"
                );
            }
            flows.complete(r, true, now);
            let key = flows.flows.keys().next().unwrap().clone();
            let deadline = flows.flows[&key].deadline();
            let raw =
                ipv4::test_icmp_error(b.ipv4.parse().unwrap(), translated_src, 3, 3, &wire[..28]);
            let error = ipv4::validate_forwarded(&raw).unwrap();
            let (target, rewritten, r) = flows
                .lookup_reply_test(&error, &b, now + Duration::from_secs(59))
                .unwrap();
            assert_eq!(target, a.id);
            assert_eq!(&rewritten[12..16], &frontend.octets());
            assert_eq!(
                &rewritten[16..20],
                &a.ipv4.parse::<Ipv4Addr>().unwrap().octets()
            );
            assert_eq!(
                &rewritten[40..44],
                &a.ipv4.parse::<Ipv4Addr>().unwrap().octets()
            );
            assert_eq!(&rewritten[44..48], &frontend.octets());
            assert!(
                r.is_none(),
                "related ICMP lookup does not allocate a connection reservation"
            );
            assert_eq!(
                flows.flows[&key].deadline(),
                deadline,
                "ICMP does not refresh business-flow idle time"
            );
            assert!(flows.has_reply_mapping(&error, &b, now));
            for bad_peer in [
                peer("different-id", "10.77.0.3"),
                Peer {
                    public_key: "different-key".into(),
                    ..b.clone()
                },
            ] {
                assert!(flows.lookup_reply_test(&error, &bad_peer, now).is_none());
            }
            let mut wrong_wire = wire.clone();
            wrong_wire[22..24].copy_from_slice(&444u16.to_be_bytes());
            let wrong = ipv4::validate_forwarded(&ipv4::test_icmp_error(
                b.ipv4.parse().unwrap(),
                translated_src,
                3,
                3,
                &wrong_wire[..28],
            ))
            .unwrap();
            assert!(flows.lookup_reply_test(&wrong, &b, now).is_none());
            assert!(flows.lookup_reply_test(&error, &b, deadline).is_none());
            assert!(!flows.has_reply_mapping(&error, &b, deadline));
        }
    }
}

#[test]
fn tcp_syn_payload_can_be_accepted_or_declined_without_losing_establishment() {
    let a = peer("a", "10.77.0.2");
    let b = peer("b", "10.77.0.3");
    let now = t();
    for accepted in [false, true] {
        let syn = tcp_numbers(
            packet(
                6,
                a.ipv4.parse().unwrap(),
                1234,
                b.ipv4.parse().unwrap(),
                443,
                2,
            ),
            100,
            0,
        );
        let mut bytes = syn.bytes().to_vec();
        bytes.extend_from_slice(b"fast open");
        let len = bytes.len() as u16;
        bytes[2..4].copy_from_slice(&len.to_be_bytes());
        bytes[10..12].fill(0);
        bytes[36..38].fill(0);
        let sum = checksum(&bytes[..20]);
        bytes[10..12].copy_from_slice(&sum.to_be_bytes());
        let sum = tcp_checksum(syn.src().octets(), syn.dst().octets(), &bytes[20..]);
        bytes[36..38].copy_from_slice(&sum.to_be_bytes());
        let syn = ipv4::validate_forwarded(&bytes).unwrap();
        let mut flows = Flows::default();
        let (_, r) = flows.prepare_direct_test(&syn, &a, &b, now).unwrap();
        flows.complete(r, true, now);
        let client_next = if accepted { 110 } else { 101 };
        let syn_ack = tcp_numbers(
            packet(
                6,
                b.ipv4.parse().unwrap(),
                443,
                a.ipv4.parse().unwrap(),
                1234,
                0x12,
            ),
            200,
            client_next,
        );
        let (_, _, r) = flows.lookup_reply_test(&syn_ack, &b, now).unwrap();
        flows.complete(r.expect("connection reply reservation"), true, now);
        let ack = tcp_numbers(
            packet(
                6,
                a.ipv4.parse().unwrap(),
                1234,
                b.ipv4.parse().unwrap(),
                443,
                0x10,
            ),
            client_next,
            201,
        );
        let (_, r) = flows.prepare_direct_test(&ack, &a, &b, now).unwrap();
        flows.complete(r, true, now);
        assert_eq!(
            flows.flows.values().next().unwrap().state.tcp_state(),
            Some(TcpState::Established)
        );
        flows.expire(now + Duration::from_secs(301));
        assert_eq!(flows.flows.len(), 1);
    }
}

#[test]
fn direct_flow_uses_destination_identity_and_exact_reverse_tuple() {
    let mut flows = Flows::default();
    let src = peer("source", "10.77.0.2");
    let dst = peer("destination", "10.77.0.3");
    let now = t();
    let p = packet(
        17,
        "10.77.0.2".parse().unwrap(),
        1234,
        "10.77.0.3".parse().unwrap(),
        5678,
        0,
    );
    let (_, r) = flows.prepare_direct_test(&p, &src, &dst, now).unwrap();
    assert!(flows
        .lookup_reply_test(
            &packet(
                17,
                "10.77.0.3".parse().unwrap(),
                5678,
                "10.77.0.2".parse().unwrap(),
                1234,
                0
            ),
            &dst,
            now
        )
        .is_none());
    flows.complete(r, true, now);
    let (target, _, r) = flows
        .lookup_reply_test(
            &packet(
                17,
                "10.77.0.3".parse().unwrap(),
                5678,
                "10.77.0.2".parse().unwrap(),
                1234,
                0,
            ),
            &dst,
            now,
        )
        .unwrap();
    assert_eq!(target, "source");
    flows.complete(
        r.expect("connection reply reservation"),
        true,
        now + Duration::from_secs(10),
    );
    for altered in [
        ("wrong-peer", "10.77.0.3", 5678, "10.77.0.2", 1234),
        ("destination", "10.77.0.4", 5678, "10.77.0.2", 1234),
        ("destination", "10.77.0.3", 5679, "10.77.0.2", 1234),
        ("destination", "10.77.0.3", 5678, "10.77.0.2", 1235),
    ] {
        let peer = peer(
            altered.0,
            if altered.0 == "destination" {
                "10.77.0.3"
            } else {
                "10.77.0.9"
            },
        );
        let p = packet(
            17,
            altered.1.parse().unwrap(),
            altered.2,
            altered.3.parse().unwrap(),
            altered.4,
            0,
        );
        assert!(flows
            .lookup_reply_test(&p, &peer, now + Duration::from_secs(11))
            .is_none());
    }
    assert!(
        flows
            .prepare_direct_test(
                &packet(
                    6,
                    "10.77.0.2".parse().unwrap(),
                    1234,
                    "10.77.0.3".parse().unwrap(),
                    5678,
                    2
                ),
                &src,
                &dst,
                now
            )
            .is_some(),
        "direct TCP SYN can initiate a tracked flow"
    );
}

#[test]
fn direct_tcp_one_way_acl_tracks_reply_but_denies_reverse_syn() {
    let mut flows = Flows::default();
    let source = peer("source", "10.77.0.2");
    let destination = peer("destination", "10.77.0.3");
    let now = t();
    let syn = packet(
        6,
        "10.77.0.2".parse().unwrap(),
        1234,
        "10.77.0.3".parse().unwrap(),
        443,
        0x02,
    );
    let (_, reservation) = flows
        .prepare_direct_test(&syn, &source, &destination, now)
        .expect("one-way ACL permits initiating direct TCP SYN");
    flows.complete(reservation, true, now);

    for flags in [0x12, 0x10] {
        // SYN/ACK reply, then continuation ACK
        let reply = packet(
            6,
            "10.77.0.3".parse().unwrap(),
            443,
            "10.77.0.2".parse().unwrap(),
            1234,
            flags,
        );
        let (target, _, reservation) = flows
            .lookup_reply_test(&reply, &destination, now)
            .expect("exact reverse TCP tuple is authorized");
        assert_eq!(target, "source");
        flows.complete(
            reservation.expect("connection reply reservation"),
            true,
            now,
        );
    }
    let reverse_syn = packet(
        6,
        "10.77.0.3".parse().unwrap(),
        443,
        "10.77.0.2".parse().unwrap(),
        1234,
        0x02,
    );
    assert!(
        flows
            .lookup_reply_test(&reverse_syn, &destination, now)
            .is_none(),
        "reverse bare SYN is a new initiation, not a reply"
    );
    let wrong_tuple = packet(
        6,
        "10.77.0.3".parse().unwrap(),
        444,
        "10.77.0.2".parse().unwrap(),
        1234,
        0x12,
    );
    assert!(flows
        .lookup_reply_test(&wrong_tuple, &destination, now)
        .is_none());
}

#[test]
fn forward_translation_identity_and_reverse_success_refresh_boundary() {
    let mut flows = Flows::default();
    let source = peer("source", "10.77.0.2");
    let backend = peer("backend", "10.77.0.3");
    let f = forward("udp", 9000);
    let now = t();
    let p = packet(
        17,
        "10.77.0.2".parse().unwrap(),
        1234,
        "10.77.0.1".parse().unwrap(),
        9000,
        0,
    );
    let (translated, r) = flows
        .prepare_forward_packet_test(&p, &source, &f, &backend, now)
        .unwrap();
    assert_eq!(&translated[12..16], &[10, 77, 0, 1]);
    assert_eq!(u16::from_be_bytes([translated[22], translated[23]]), 9000);
    let snat = u16::from_be_bytes([translated[20], translated[21]]);
    flows.complete(r, true, now);
    let reply = packet(
        17,
        "10.77.0.3".parse().unwrap(),
        9000,
        "10.77.0.1".parse().unwrap(),
        snat,
        0,
    );
    for (peer, src, sport, dst, dport) in [
        (&backend, "10.77.0.4", 9000, "10.77.0.1", snat),
        (&backend, "10.77.0.3", 9001, "10.77.0.1", snat),
        (&backend, "10.77.0.3", 9000, "10.77.0.2", snat),
        (&backend, "10.77.0.3", 9000, "10.77.0.1", snat + 1),
        (&source, "10.77.0.3", 9000, "10.77.0.1", snat),
    ] {
        let p = packet(
            17,
            src.parse().unwrap(),
            sport,
            dst.parse().unwrap(),
            dport,
            0,
        );
        assert!(flows.lookup_reply_test(&p, peer, now).is_none());
    }
    let (target, rewritten, r) = flows
        .lookup_reply_test(&reply, &backend, now + Duration::from_secs(59))
        .unwrap();
    assert_eq!(target, "source");
    assert_eq!(&rewritten[12..16], &[10, 77, 0, 1]);
    assert_eq!(u16::from_be_bytes([rewritten[22], rewritten[23]]), 1234);
    flows.complete(
        r.expect("connection reply reservation"),
        true,
        now + Duration::from_secs(59),
    );
    assert!(flows
        .lookup_reply_test(&reply, &backend, now + Duration::from_secs(118))
        .is_some());
    assert!(flows
        .lookup_reply_test(&reply, &backend, now + Duration::from_secs(119))
        .is_none());
    assert!(flows.reverse.is_empty());
}

#[test]
fn failed_initial_delivery_does_not_grant_reply_access_or_refresh_active_flow() {
    let mut flows = Flows::default();
    let source = peer("source", "10.77.0.2");
    let backend = peer("backend", "10.77.0.3");
    let now = t();
    let f = forward("udp", 9000);
    let p = packet(
        17,
        "10.77.0.2".parse().unwrap(),
        1234,
        "10.77.0.1".parse().unwrap(),
        9000,
        0,
    );
    let (_, r) = flows
        .prepare_forward_packet_test(&p, &source, &f, &backend, now)
        .unwrap();
    assert!(flows
        .lookup_reply_test(
            &packet(
                17,
                "10.77.0.3".parse().unwrap(),
                9000,
                "10.77.0.1".parse().unwrap(),
                40000,
                0
            ),
            &backend,
            now
        )
        .is_none());
    flows.complete(r, false, now);
    assert!(
        flows.pending.is_empty() && flows.pending_reverse.is_empty() && flows.reverse.is_empty()
    );

    let (_, r) = flows
        .prepare_forward_packet_test(&p, &source, &f, &backend, now)
        .unwrap();
    flows.complete(r, true, now);
    let (_, _, r) = flows
        .lookup_reply_test(
            &packet(
                17,
                "10.77.0.3".parse().unwrap(),
                9000,
                "10.77.0.1".parse().unwrap(),
                40001,
                0,
            ),
            &backend,
            now + Duration::from_secs(30),
        )
        .unwrap();
    flows.complete(
        r.expect("connection reply reservation"),
        false,
        now + Duration::from_secs(30),
    );
    assert!(flows
        .lookup_reply_test(
            &packet(
                17,
                "10.77.0.3".parse().unwrap(),
                9000,
                "10.77.0.1".parse().unwrap(),
                40001,
                0
            ),
            &backend,
            now + Duration::from_secs(60)
        )
        .is_none());
}

#[test]
fn expired_forward_reclaimed_and_tcp_requires_syn_only_for_new_record() {
    let mut flows = Flows::default();
    let source = peer("source", "10.77.0.2");
    let backend = peer("backend", "10.77.0.3");
    let f = forward("tcp", 443);
    let now = t();
    let syn = packet(
        6,
        "10.77.0.2".parse().unwrap(),
        1234,
        "10.77.0.1".parse().unwrap(),
        443,
        2,
    );
    let (_, r) = flows
        .prepare_forward_packet_test(&syn, &source, &f, &backend, now)
        .unwrap();
    flows.complete(r, true, now);
    let ack = packet(
        6,
        "10.77.0.2".parse().unwrap(),
        1234,
        "10.77.0.1".parse().unwrap(),
        443,
        16,
    );
    assert!(flows
        .prepare_forward_packet_test(&ack, &source, &f, &backend, now + Duration::from_secs(59))
        .is_some());
    // Complete that existing delivery so its reservation does not affect pending state.
    let (_, r) = flows
        .prepare_forward_packet_test(&ack, &source, &f, &backend, now + Duration::from_secs(59))
        .unwrap();
    flows.complete(r, true, now + Duration::from_secs(59));
    assert!(flows
        .prepare_forward_packet_test(&ack, &source, &f, &backend, now + Duration::from_secs(120))
        .is_none());
    assert!(flows.flows.is_empty() && flows.reverse.is_empty());
    assert!(flows
        .prepare_forward_packet_test(&syn, &source, &f, &backend, now + Duration::from_secs(120))
        .is_some());
}

#[test]
fn pending_dedup_service_exclusion_protocol_sharing_and_capacity() {
    let backend = peer("backend", "10.77.0.3");
    let source = peer("source", "10.77.0.2");
    let mut flows = Flows::new("10.77.0.1".parse().unwrap(), &[forward("udp", 40000)]);
    let now = t();
    let udp_forward = forward("udp", 53);
    let udp = packet(
        17,
        "10.77.0.2".parse().unwrap(),
        1111,
        "10.77.0.1".parse().unwrap(),
        53,
        0,
    );
    let (first, reservation) = flows
        .prepare_forward_packet_test(&udp, &source, &udp_forward, &backend, now)
        .unwrap();
    assert_eq!(u16::from_be_bytes([first[20], first[21]]), 40001);
    assert!(flows
        .prepare_forward_packet_test(&udp, &source, &udp_forward, &backend, now)
        .is_none());
    // Identical numeric port may be used independently by TCP.
    let tcp_forward = forward("tcp", 53);
    let tcp = packet(
        6,
        "10.77.0.2".parse().unwrap(),
        1111,
        "10.77.0.1".parse().unwrap(),
        53,
        2,
    );
    let (translated_tcp, tcp_reservation) = flows
        .prepare_forward_packet_test(&tcp, &source, &tcp_forward, &backend, now)
        .unwrap();
    assert_eq!(
        u16::from_be_bytes([translated_tcp[20], translated_tcp[21]]),
        40000
    );
    flows.complete(reservation, false, now);
    flows.complete(tcp_reservation, true, now);

    let mut full = Flows::default();
    for n in 0..CAPACITY {
        let key = Tuple {
            peer: format!("p{n}"),
            ip: Ipv4Addr::LOCALHOST,
            port: n as u16,
            frontend_ip: Ipv4Addr::LOCALHOST,
            frontend_port: 9,
            protocol: 17,
        };
        let flow = Flow {
            reply: Reverse {
                peer: format!("b{n}"),
                proto: 17,
                src: Ipv4Addr::LOCALHOST,
                sport: 9,
                dst: Ipv4Addr::LOCALHOST,
                dport: n as u16,
            },
            output: None,
            backend: "backend".into(),
            backend_ip: Ipv4Addr::LOCALHOST,
            initiator_key: PeerKey::Invalid(String::new()),
            backend_key: PeerKey::Invalid(String::new()),
            forward_id: None,
            last: now,
            generation: n as u64,
            state: ConnectionEvent::Udp.initial_state().unwrap(),
        };
        full.pending_reverse.insert(flow.reply.clone(), key.clone());
        full.pending.insert(key, flow);
    }
    assert!(full.reserve_capacity("capacity-check", now).is_none());

    let mut exhausted = Flows::default();
    for port in SNAT_START..=SNAT_END {
        exhausted.pending_reverse.insert(
            Reverse {
                peer: backend.id.clone(),
                proto: 17,
                src: backend.ipv4.parse().unwrap(),
                sport: 53,
                dst: exhausted.hub_ip,
                dport: port,
            },
            Tuple {
                peer: format!("used{port}"),
                ip: Ipv4Addr::LOCALHOST,
                port,
                frontend_ip: Ipv4Addr::LOCALHOST,
                frontend_port: 53,
                protocol: 17,
            },
        );
    }
    assert!(exhausted.choose_snat(17, &backend, 53).is_none());
}

#[test]
fn per_initiator_quota_is_shared_by_protocols_and_released_on_cleanup() {
    let backend = peer("backend", "10.77.0.3");
    let source = peer("source", "10.77.0.2");
    let other = peer("other", "10.77.0.4");
    let udp_forward = forward("udp", 53);
    let tcp_forward = forward("tcp", 443);
    let now = t();
    let mut flows = Flows::default();

    // Active UDP and pending TCP both count; distinct source ports cannot
    // evade the per-initiator limit, and a different initiator is unaffected.
    for port in 0..(PEER_CAPACITY / 2) {
        let p = packet(
            17,
            source.ipv4.parse().unwrap(),
            port as u16,
            flows.hub_ip,
            53,
            0,
        );
        let (_, r) = flows
            .prepare_forward_packet_test(&p, &source, &udp_forward, &backend, now)
            .unwrap();
        flows.complete(r, true, now);
    }
    for port in (PEER_CAPACITY / 2)..PEER_CAPACITY {
        let p = packet(
            6,
            source.ipv4.parse().unwrap(),
            port as u16,
            flows.hub_ip,
            443,
            2,
        );
        assert!(flows
            .prepare_forward_packet_test(&p, &source, &tcp_forward, &backend, now)
            .is_some());
    }
    let denied = packet(
        17,
        source.ipv4.parse().unwrap(),
        10_000,
        flows.hub_ip,
        53,
        0,
    );
    assert!(flows
        .prepare_forward_packet_test(&denied, &source, &udp_forward, &backend, now)
        .is_none());
    let allowed = packet(17, other.ipv4.parse().unwrap(), 10_000, flows.hub_ip, 53, 0);
    assert!(flows
        .prepare_forward_packet_test(&allowed, &other, &udp_forward, &backend, now)
        .is_some());

    // Repeated traffic on an already-active key refreshes without quota.
    let existing = packet(17, source.ipv4.parse().unwrap(), 0, flows.hub_ip, 53, 0);
    assert!(flows
        .prepare_forward_packet_test(&existing, &source, &udp_forward, &backend, now)
        .is_some());

    // Failed pending delivery releases a slot immediately.
    let pending_key = Tuple {
        peer: source.id.clone(),
        ip: source.ipv4.parse().unwrap(),
        port: (PEER_CAPACITY / 2) as u16,
        frontend_ip: flows.hub_ip,
        frontend_port: 443,
        protocol: 6,
    };
    let generation = flows.pending.get(&pending_key).unwrap().generation;
    flows.complete(
        Reservation {
            key: pending_key,
            generation,
            is_new: true,
            from_initiator: true,
            event: ConnectionEvent::Udp,
        },
        false,
        now,
    );
    let replacement = packet(
        6,
        source.ipv4.parse().unwrap(),
        PEER_CAPACITY as u16,
        flows.hub_ip,
        443,
        2,
    );
    assert!(flows
        .prepare_forward_packet_test(&replacement, &source, &tcp_forward, &backend, now)
        .is_some());

    // Expiration, reconcile, and clear all restore admission capacity.
    flows.expire(now + Duration::from_secs(301));
    let after_expiry = packet(
        17,
        source.ipv4.parse().unwrap(),
        11_000,
        flows.hub_ip,
        53,
        0,
    );
    assert!(flows
        .prepare_forward_packet_test(
            &after_expiry,
            &source,
            &udp_forward,
            &backend,
            now + Duration::from_secs(301)
        )
        .is_some());
    flows.reconcile(Some(flows.hub_ip), Some(flows.hub_ip), &[], &[], |_| None);
    assert!(flows.flows.is_empty() && flows.pending.is_empty());
    assert!(flows
        .prepare_forward_packet_test(
            &after_expiry,
            &source,
            &udp_forward,
            &backend,
            now + Duration::from_secs(302)
        )
        .is_some());
    flows.clear();
    assert!(flows
        .prepare_forward_packet_test(
            &after_expiry,
            &source,
            &udp_forward,
            &backend,
            now + Duration::from_secs(303)
        )
        .is_some());
}

#[test]
fn direct_quota_sweeps_peer_expiry_and_ignores_late_reservations() {
    let source = peer("source", "10.77.0.2");
    let destination = peer("destination", "10.77.0.3");
    let now = t();
    let mut flows = Flows::default();
    let mut last_reservation = None;

    for port in 0..PEER_CAPACITY {
        let p = packet(
            17,
            source.ipv4.parse().unwrap(),
            port as u16,
            destination.ipv4.parse().unwrap(),
            9000,
            0,
        );
        let (_, reservation) = flows
            .prepare_direct_test(&p, &source, &destination, now)
            .unwrap();
        if port + 1 == PEER_CAPACITY {
            last_reservation = Some(reservation);
        } else {
            flows.complete(reservation, true, now);
        }
    }
    assert_eq!(flows.peer_counts.get(&source.id), Some(&PEER_CAPACITY));

    // Pending state also consumes quota. Expire it at the per-peer
    // boundary while the global table is nowhere near full.
    let pending_packet = packet(
        17,
        source.ipv4.parse().unwrap(),
        1000,
        destination.ipv4.parse().unwrap(),
        9000,
        0,
    );
    assert!(flows
        .prepare_direct_test(&pending_packet, &source, &destination, now)
        .is_none());
    flows.expire(now + UDP_IDLE);
    let reservation = last_reservation.unwrap();
    flows.complete(reservation, true, now + UDP_IDLE);
    assert!(flows.flows.is_empty());
    assert!(!flows.peer_counts.contains_key(&source.id));

    // Admission itself performs the same targeted cleanup; no separate
    // periodic global sweep is required for a full peer.
    let p = packet(
        17,
        source.ipv4.parse().unwrap(),
        1001,
        destination.ipv4.parse().unwrap(),
        9000,
        0,
    );
    assert!(flows
        .prepare_direct_test(&p, &source, &destination, now + UDP_IDLE)
        .is_some());
}

#[test]
fn quota_full_without_expiry_does_not_rescan_table_per_attempt() {
    let mut flows = Flows::default();
    let now = t();
    for peer_id in ["p1", "p2"] {
        for port in 0..PEER_CAPACITY {
            let key = Tuple {
                peer: peer_id.into(),
                ip: Ipv4Addr::LOCALHOST,
                port: port as u16,
                frontend_ip: Ipv4Addr::LOCALHOST,
                frontend_port: 9,
                protocol: 17,
            };
            let flow = Flow {
                reply: Reverse {
                    peer: "backend".into(),
                    proto: 17,
                    src: Ipv4Addr::LOCALHOST,
                    sport: 9,
                    dst: Ipv4Addr::LOCALHOST,
                    dport: port as u16,
                },
                output: None,
                backend: "backend".into(),
                backend_ip: Ipv4Addr::LOCALHOST,
                initiator_key: PeerKey::Invalid(String::new()),
                backend_key: PeerKey::Invalid(String::new()),
                forward_id: None,
                last: now,
                generation: port as u64,
                state: ConnectionEvent::Udp.initial_state().unwrap(),
            };
            flows.flows.insert(key, flow);
        }
    }
    flows.rebuild_peer_counts();
    flows.recompute_earliest_expiry();
    let before = flows.expiry_scans;
    for _ in 0..12 {
        assert!(flows.reserve_capacity("p1", now).is_none());
    }
    assert_eq!(flows.expiry_scans, before);
    assert!(
        flows.reserve_capacity("p1", now + UDP_IDLE).is_some(),
        "at expiry boundary a sweep must immediately reclaim quota"
    );
    assert_eq!(flows.expiry_scans, before + 1);
}

#[test]
fn tcp_fin_and_rst_use_fixed_close_grace_and_successful_delivery_only() {
    let mut flows = Flows::default();
    let source = peer("source", "10.77.0.2");
    let backend = peer("backend", "10.77.0.3");
    let tcp_forward = forward("tcp", 443);
    let udp_forward = forward("udp", 53);
    let now = t();
    let syn = packet(
        6,
        source.ipv4.parse().unwrap(),
        1234,
        flows.hub_ip,
        443,
        0x02,
    );
    let (translated, reservation) = flows
        .prepare_forward_packet_test(&syn, &source, &tcp_forward, &backend, now)
        .unwrap();
    let snat = u16::from_be_bytes([translated[20], translated[21]]);
    flows.complete(reservation, true, now);
    let key = flows.flows.keys().next().unwrap().clone();

    // An undelivered FIN is not lifecycle state.
    let failed_fin = packet(
        6,
        source.ipv4.parse().unwrap(),
        1234,
        flows.hub_ip,
        443,
        0x11,
    );
    let (_, reservation) = flows
        .prepare_forward_packet_test(
            &failed_fin,
            &source,
            &tcp_forward,
            &backend,
            now + Duration::from_secs(1),
        )
        .unwrap();
    flows.complete(reservation, false, now + Duration::from_secs(1));
    assert_eq!(flows.flows[&key].state.fin_directions(), 0);

    // One successful FIN leaves the opposite direction available and uses normal idle timeout.
    let (_, reservation) = flows
        .prepare_forward_packet_test(
            &failed_fin,
            &source,
            &tcp_forward,
            &backend,
            now + Duration::from_secs(2),
        )
        .unwrap();
    flows.complete(reservation, true, now + Duration::from_secs(2));
    assert_eq!(flows.flows[&key].state.fin_directions(), 1);
    let same_syn = packet(
        6,
        source.ipv4.parse().unwrap(),
        1234,
        flows.hub_ip,
        443,
        0x02,
    );
    assert!(flows
        .prepare_forward_packet_test(
            &same_syn,
            &source,
            &tcp_forward,
            &backend,
            now + Duration::from_secs(3)
        )
        .is_none());
    assert!(flows
        .lookup_reply_test(
            &packet(
                6,
                "10.77.0.3".parse().unwrap(),
                443,
                flows.hub_ip,
                snat,
                0x10
            ),
            &backend,
            now + Duration::from_secs(40)
        )
        .is_some());

    // Prepare both directions before either completion, then complete in reverse order.
    let close_at = now + Duration::from_secs(41);
    let (_, initiator_fin) = flows
        .prepare_forward_packet_test(&failed_fin, &source, &tcp_forward, &backend, close_at)
        .unwrap();
    let reply_fin = packet(
        6,
        "10.77.0.3".parse().unwrap(),
        443,
        flows.hub_ip,
        snat,
        0x11,
    );
    let (_, _, reverse_fin) = flows
        .lookup_reply_test(&reply_fin, &backend, close_at)
        .unwrap();
    let (_, old_ack) = flows
        .prepare_forward_packet_test(
            &packet(
                6,
                source.ipv4.parse().unwrap(),
                1234,
                flows.hub_ip,
                443,
                0x10,
            ),
            &source,
            &tcp_forward,
            &backend,
            close_at,
        )
        .unwrap();
    flows.complete(
        reverse_fin.expect("connection reply reservation"),
        true,
        close_at,
    );
    flows.complete(initiator_fin, true, close_at);
    assert_eq!(flows.flows[&key].state.fin_directions(), 3);
    assert_eq!(
        flows.flows[&key].state.closed_until(),
        Some(close_at + TCP_CLOSED_GRACE)
    );
    flows.complete(old_ack, true, close_at + Duration::from_secs(1));
    assert_eq!(
        flows.flows[&key].state.closed_until(),
        Some(close_at + TCP_CLOSED_GRACE)
    );
    assert!(flows
        .prepare_forward_packet_test(
            &same_syn,
            &source,
            &tcp_forward,
            &backend,
            close_at + Duration::from_secs(2)
        )
        .is_none());

    // Replays cannot extend the fixed grace deadline.
    let (_, _, reservation) = flows
        .lookup_reply_test(&reply_fin, &backend, close_at)
        .unwrap();
    flows.complete(
        reservation.expect("connection reply reservation"),
        true,
        close_at,
    );
    let retransmit_at = close_at + Duration::from_secs(20);
    let (_, _, reservation) = flows
        .lookup_reply_test(&reply_fin, &backend, retransmit_at)
        .unwrap();
    flows.complete(
        reservation.expect("connection reply reservation"),
        true,
        retransmit_at,
    );
    assert_eq!(
        flows.flows[&key].state.closed_until(),
        Some(close_at + TCP_CLOSED_GRACE)
    );
    assert!(flows
        .lookup_reply_test(&reply_fin, &backend, close_at + TCP_CLOSED_GRACE)
        .is_none());
    assert!(!flows.reverse.values().any(|mapped| mapped == &key));

    // RST has the same grace; after expiry the TCP tuple can be initiated again and UDP quota is free.
    let rst_key_port = 1235;
    let rst_syn = packet(
        6,
        source.ipv4.parse().unwrap(),
        rst_key_port,
        flows.hub_ip,
        443,
        0x02,
    );
    let (_, reservation) = flows
        .prepare_forward_packet_test(
            &rst_syn,
            &source,
            &tcp_forward,
            &backend,
            close_at + TCP_CLOSED_GRACE,
        )
        .unwrap();
    flows.complete(reservation, true, close_at + TCP_CLOSED_GRACE);
    let rst = packet(
        6,
        source.ipv4.parse().unwrap(),
        rst_key_port,
        flows.hub_ip,
        443,
        0x04,
    );
    let rst_at = close_at + TCP_CLOSED_GRACE + Duration::from_secs(1);
    let (_, reservation) = flows
        .prepare_forward_packet_test(&rst, &source, &tcp_forward, &backend, rst_at)
        .unwrap();
    flows.complete(reservation, true, rst_at);
    let rst_key = flows
        .flows
        .keys()
        .find(|key| key.port == rst_key_port)
        .unwrap()
        .clone();
    assert_eq!(
        flows.flows[&rst_key].state.closed_until(),
        Some(rst_at + TCP_CLOSED_GRACE)
    );
    assert!(flows
        .prepare_forward_packet_test(
            &rst_syn,
            &source,
            &tcp_forward,
            &backend,
            rst_at + Duration::from_secs(1)
        )
        .is_none());
    let rst_replay_at = rst_at + Duration::from_secs(20);
    let (_, reservation) = flows
        .prepare_forward_packet_test(&rst, &source, &tcp_forward, &backend, rst_replay_at)
        .unwrap();
    flows.complete(reservation, true, rst_replay_at);
    assert_eq!(
        flows.flows[&rst_key].state.closed_until(),
        Some(rst_at + TCP_CLOSED_GRACE)
    );
    let freed_at = rst_at + TCP_CLOSED_GRACE;
    flows.expire(freed_at);
    assert!(!flows.flows.contains_key(&rst_key));
    let new_syn = packet(
        6,
        source.ipv4.parse().unwrap(),
        1236,
        flows.hub_ip,
        443,
        0x02,
    );
    assert!(flows
        .prepare_forward_packet_test(&new_syn, &source, &tcp_forward, &backend, freed_at)
        .is_some());
    let reused_tuple_syn = packet(
        6,
        source.ipv4.parse().unwrap(),
        rst_key_port,
        flows.hub_ip,
        443,
        0x02,
    );
    assert!(flows
        .prepare_forward_packet_test(&reused_tuple_syn, &source, &tcp_forward, &backend, freed_at)
        .is_some());
    let udp = packet(17, source.ipv4.parse().unwrap(), 9999, flows.hub_ip, 53, 0);
    assert!(flows
        .prepare_forward_packet_test(&udp, &source, &udp_forward, &backend, freed_at)
        .is_some());
}

#[test]
fn rst_releases_full_peer_quota_after_short_grace_for_tcp_and_udp() {
    let mut flows = Flows::default();
    let source = peer("source", "10.77.0.2");
    let backend = peer("backend", "10.77.0.3");
    let tcp_forward = forward("tcp", 443);
    let udp_forward = forward("udp", 53);
    let now = t();
    for port in 0..PEER_CAPACITY {
        let syn = packet(
            6,
            source.ipv4.parse().unwrap(),
            port as u16,
            flows.hub_ip,
            443,
            0x02,
        );
        let (_, reservation) = flows
            .prepare_forward_packet_test(&syn, &source, &tcp_forward, &backend, now)
            .unwrap();
        flows.complete(reservation, true, now);
    }
    assert_eq!(flows.peer_counts.get(&source.id), Some(&PEER_CAPACITY));
    for port in 0..PEER_CAPACITY {
        let rst = packet(
            6,
            source.ipv4.parse().unwrap(),
            port as u16,
            flows.hub_ip,
            443,
            0x04,
        );
        let (_, reservation) = flows
            .prepare_forward_packet_test(
                &rst,
                &source,
                &tcp_forward,
                &backend,
                now + Duration::from_secs(1),
            )
            .unwrap();
        flows.complete(reservation, true, now + Duration::from_secs(1));
    }
    let boundary = now + Duration::from_secs(1) + TCP_CLOSED_GRACE;
    let new_syn = packet(
        6,
        source.ipv4.parse().unwrap(),
        1000,
        flows.hub_ip,
        443,
        0x02,
    );
    let new_udp = packet(17, source.ipv4.parse().unwrap(), 1001, flows.hub_ip, 53, 0);
    assert!(flows
        .prepare_forward_packet_test(&new_syn, &source, &tcp_forward, &backend, boundary)
        .is_some());
    assert!(flows
        .prepare_forward_packet_test(&new_udp, &source, &udp_forward, &backend, boundary)
        .is_some());
    assert_eq!(flows.peer_counts.get(&source.id), Some(&2));
}

#[test]
fn stale_active_reservations_cannot_resurrect_or_replace_a_new_generation() {
    let source = peer("source", "10.77.0.2");
    let backend = peer("backend", "10.77.0.3");
    let f = forward("udp", 53);
    let now = t();
    let packet = packet(
        17,
        source.ipv4.parse().unwrap(),
        1234,
        "10.77.0.1".parse().unwrap(),
        53,
        0,
    );

    for invalidation in [0, 1, 2] {
        let mut flows = Flows::default();
        let (_, first) = flows
            .prepare_forward_packet_test(&packet, &source, &f, &backend, now)
            .unwrap();
        flows.complete(first, true, now);
        let (_, stale) = flows
            .prepare_forward_packet_test(
                &packet,
                &source,
                &f,
                &backend,
                now + Duration::from_secs(1),
            )
            .unwrap();
        match invalidation {
            0 => flows.expire(now + UDP_IDLE),
            1 => flows.clear(),
            _ => flows.reconcile(Some(flows.hub_ip), Some(flows.hub_ip), &[], &[], |_| None),
        }
        flows.complete(stale, true, now + UDP_IDLE);
        assert!(flows.flows.is_empty() && flows.reverse.is_empty() && flows.peer_counts.is_empty());
    }

    let mut flows = Flows::default();
    let (_, first) = flows
        .prepare_forward_packet_test(&packet, &source, &f, &backend, now)
        .unwrap();
    flows.complete(first, true, now);
    let (_, stale) = flows
        .prepare_forward_packet_test(&packet, &source, &f, &backend, now + Duration::from_secs(1))
        .unwrap();
    let old_generation = stale.generation;
    let old_snat = flows.flows[&stale.key].reply.dport;
    flows.expire(now + UDP_IDLE);
    let (_, fresh) = flows
        .prepare_forward_packet_test(&packet, &source, &f, &backend, now + UDP_IDLE)
        .unwrap();
    let fresh_generation = fresh.generation;
    let fresh_snat = flows.pending[&fresh.key].reply.dport;
    assert_ne!(old_generation, fresh_generation);
    assert_ne!(old_snat, fresh_snat);
    flows.complete(fresh, true, now + UDP_IDLE);
    let count = flows.peer_counts.get(&source.id).copied();
    flows.complete(stale, true, now + UDP_IDLE + Duration::from_secs(1));
    assert_eq!(
        flows.flows[&Tuple {
            peer: source.id.clone(),
            ip: source.ipv4.parse().unwrap(),
            port: 1234,
            frontend_ip: flows.hub_ip,
            frontend_port: 53,
            protocol: 17
        }]
            .generation,
        fresh_generation
    );
    assert_eq!(flows.reverse.len(), 1);
    assert_eq!(flows.flows.values().next().unwrap().reply.dport, fresh_snat);
    assert_eq!(flows.peer_counts.get(&source.id).copied(), count);
}

#[test]
fn service_port_collision_removes_only_matching_protocol_snat_mapping() {
    let mut flows = Flows::default();
    let source = "source".to_string();
    let peer_ip = Ipv4Addr::new(10, 77, 0, 2);
    let hub_ip = flows.hub_ip;
    for (port, proto, snat) in [(1234, 17, 40000), (1235, 17, 40001), (1236, 6, 40000)] {
        let key = Tuple {
            peer: source.clone(),
            ip: peer_ip,
            port,
            frontend_ip: hub_ip,
            frontend_port: 9000,
            protocol: proto,
        };
        let reply = Reverse {
            peer: "backend".into(),
            proto,
            src: "10.77.0.3".parse().unwrap(),
            sport: 9000,
            dst: hub_ip,
            dport: snat,
        };
        let flow = Flow {
            reply: reply.clone(),
            output: Some(PacketTuple {
                src: hub_ip,
                src_port: snat,
                dst: reply.src,
                dst_port: 9000,
            }),
            backend: "backend".into(),
            backend_ip: reply.src,
            initiator_key: PeerKey::Invalid(String::new()),
            backend_key: PeerKey::Invalid(String::new()),
            forward_id: Some("f".into()),
            last: t(),
            generation: port as u64,
            state: ConnectionEvent::Udp.initial_state().unwrap(),
        };
        flows.reverse.insert(reply, key.clone());
        flows.flows.insert(key, flow);
    }
    flows.rebuild_peer_counts();
    assert_eq!(flows.peer_counts.get(&source), Some(&3));
    let service_ports = HashSet::from([(17, 40000)]);
    flows.remove_service_port_collisions(&service_ports);
    assert_eq!(flows.peer_counts.get(&source), Some(&2));
    assert!(!flows.flows.values().any(|flow| flow.reply.proto == 17
        && flow
            .output
            .as_ref()
            .is_some_and(|out| out.src_port == 40000)));
    assert!(flows.flows.values().any(|flow| flow.reply.proto == 17
        && flow
            .output
            .as_ref()
            .is_some_and(|out| out.src_port == 40001)));
    assert!(flows.flows.values().any(|flow| flow.reply.proto == 6
        && flow
            .output
            .as_ref()
            .is_some_and(|out| out.src_port == 40000)));
    assert!(!flows
        .reverse
        .keys()
        .any(|reply| reply.proto == 17 && reply.dport == 40000));
}
