//! Repeatable, opt-in release-mode routing baseline. No sockets or production state.
use super::{
    checksum::{checksum, transport_checksum},
    dataplane::{DataPlane, EgressOutcome, RoutingConfig},
    snapshot::CompiledSnapshot,
};
use crate::model::{Forward, Group, NetworkSettings, NetworkSnapshot, Peer};
use base64::Engine;
use std::{hint::black_box, net::Ipv4Addr, time::Instant};

fn plane(peers: usize, forwards: usize) -> DataPlane {
    let raw = NetworkSnapshot {
        revision: 0,
        settings: Some(NetworkSettings {
            subnet: "10.77.0.0/24".into(),
            endpoint: "localhost:51820".into(),
            persistent_keepalive: 25,
        }),
        groups: vec![Group {
            id: "g".into(),
            name: "g".into(),
            allowed_groups: vec!["g".into()],
        }],
        peers: (0..peers)
            .map(|i| {
                let mut key = [0u8; 32];
                key[..8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
                Peer {
                    id: format!("p{i}"),
                    name: format!("p{i}"),
                    public_key: base64::engine::general_purpose::STANDARD.encode(key),
                    ipv4: format!("10.77.0.{}", i + 2),
                    group_id: "g".into(),
                    received_bytes: 0,
                    sent_bytes: 0,
                    last_handshake_unix: None,
                }
            })
            .collect(),
        forwards: (0..forwards)
            .map(|i| Forward {
                id: format!("f{i}"),
                name: format!("f{i}"),
                protocol: if i % 2 == 0 { "udp" } else { "tcp" }.into(),
                target_port: (10_000 + i) as u16,
                target_peer_id: format!("p{}", peers - 1),
                allowed_group_ids: vec!["g".into()],
            })
            .collect(),
    };
    let c = CompiledSnapshot::try_from(raw).unwrap();
    let next = RoutingConfig {
        peers: c.peers,
        forwards: c.forwards,
        forward_index: c.forward_index,
        hub_ip: c.hub_ip,
        by_ip: c.peer_by_ip,
    };
    let mut dp = DataPlane::default();
    dp.reconcile(&RoutingConfig::default(), next);
    dp
}

fn packet(
    protocol: u8,
    size: usize,
    source: Ipv4Addr,
    target: Ipv4Addr,
    sport: u16,
    dport: u16,
) -> Vec<u8> {
    let mut p = vec![0u8; size];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&(size as u16).to_be_bytes());
    p[8] = 64;
    p[9] = protocol;
    p[12..16].copy_from_slice(&source.octets());
    p[16..20].copy_from_slice(&target.octets());
    if protocol == 1 {
        p[20] = 8;
        p[24..26].copy_from_slice(&sport.to_be_bytes());
        let sum = checksum(&p[20..]);
        p[22..24].copy_from_slice(&sum.to_be_bytes());
    } else {
        p[20..22].copy_from_slice(&sport.to_be_bytes());
        p[22..24].copy_from_slice(&dport.to_be_bytes());
        let at = if protocol == 6 {
            p[24..28].copy_from_slice(&1u32.to_be_bytes());
            p[32] = 0x50;
            p[33] = 2;
            p[34..36].copy_from_slice(&65535u16.to_be_bytes());
            36
        } else {
            p[24..26].copy_from_slice(&((size - 20) as u16).to_be_bytes());
            26
        };
        let sum = transport_checksum(source, target, protocol, &p[20..]);
        p[at..at + 2].copy_from_slice(&sum.to_be_bytes());
    }
    let sum = checksum(&p[..20]);
    p[10..12].copy_from_slice(&sum.to_be_bytes());
    p
}

#[test]
#[ignore = "performance baseline; run tests/performance.py in release mode"]
fn baseline() {
    assert!(
        !black_box(cfg!(debug_assertions)),
        "Performance results require --release"
    );
    let iterations: usize = std::env::var("WIREHUB_BENCH_ITERATIONS")
        .unwrap_or_else(|_| "20000".into())
        .parse()
        .unwrap();
    assert!(iterations >= 1000);
    for peers in [1, 32, 128, 253] {
        for size in [64, 512, 1280, 1420] {
            for (name, protocol) in [("icmp", 1), ("tcp", 6), ("udp", 17)] {
                for mode in ["direct", "forward"] {
                    if mode == "forward" && protocol == 1 {
                        continue;
                    }
                    let forwards = if mode == "forward" { 1024 } else { 0 };
                    let mut dp = plane(peers, forwards);
                    let source = dp.authenticated_peer("p0").unwrap();
                    let target = if mode == "forward" {
                        Ipv4Addr::new(10, 77, 0, 1)
                    } else {
                        Ipv4Addr::new(10, 77, 0, (peers + 1) as u8)
                    };
                    let port = if mode == "forward" {
                        if protocol == 6 {
                            11023
                        } else {
                            11022
                        }
                    } else {
                        8080
                    };
                    let p = packet(
                        protocol,
                        size,
                        Ipv4Addr::new(10, 77, 0, 2),
                        target,
                        1234,
                        port,
                    );
                    let now = Instant::now();
                    // Warm all paths and commit a flow before measuring repeated traffic.
                    for _ in 0..1000 {
                        let ingress = dp.ingress(&source, &p).unwrap();
                        let delivery = dp.prepare(ingress, now).unwrap();
                        assert!(dp.finish(delivery, EgressOutcome::Delivered, now).is_some());
                    }
                    let mut latency = Vec::with_capacity(iterations);
                    let start = Instant::now();
                    for _ in 0..iterations {
                        let sample = Instant::now();
                        let ingress = dp.ingress(black_box(&source), black_box(&p)).unwrap();
                        let delivery = dp.prepare(ingress, now).unwrap();
                        black_box(dp.finish(delivery, EgressOutcome::Delivered, now).unwrap());
                        latency.push(sample.elapsed().as_nanos() as u64);
                    }
                    let elapsed = start.elapsed().as_secs_f64();
                    latency.sort_unstable();
                    println!(
                        "BENCH {}",
                        serde_json::json!({"kind":"routing","peers":peers,"protocol":name,"packet_bytes":size,"mode":mode,"forwards":forwards,"iterations":iterations,"packets_per_second":iterations as f64/elapsed,"inner_mbit_per_second":iterations as f64*size as f64*8.0/elapsed/1e6,"p50_ns":latency[iterations/2],"p95_ns":latency[iterations*95/100],"p99_ns":latency[iterations*99/100]})
                    );
                }
            }
        }
    }
    // Hit both the 256-per-peer and 16,384-global flow boundaries with unique UDP tuples.
    for peers in [1, 32, 128, 253] {
        let mut dp = plane(peers, 0);
        let now = Instant::now();
        let start = Instant::now();
        let mut accepted = 0;
        let mut rejected = 0;
        for i in 0..peers {
            let source = dp.authenticated_peer(&format!("p{i}")).unwrap();
            for port in 1..=257 {
                let p = packet(
                    17,
                    64,
                    Ipv4Addr::new(10, 77, 0, (i + 2) as u8),
                    Ipv4Addr::new(10, 77, 0, 2),
                    port,
                    8080,
                );
                let ingress = dp.ingress(&source, &p).unwrap();
                if let Some(delivery) = dp.prepare(ingress, now) {
                    if dp.finish(delivery, EgressOutcome::Delivered, now).is_some() {
                        accepted += 1;
                    } else {
                        rejected += 1;
                    }
                } else {
                    rejected += 1;
                }
            }
        }
        assert_eq!(accepted, (peers * 256).min(16384));
        assert_eq!(accepted + rejected, peers * 257);
        println!(
            "BENCH {}",
            serde_json::json!({"kind":"capacity","peers":peers,"accepted":accepted,"rejected":rejected,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0})
        );
    }
}
