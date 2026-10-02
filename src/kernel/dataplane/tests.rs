use super::*;
use crate::{kernel::ipv4, model::Group};

fn peer(id: &str, ip: [u8; 4], key: u8) -> PeerConfig {
    PeerConfig {
        id: id.into(),
        key: [key; 32],
        ip: Ipv4Addr::from(ip),
        group_id: "g".into(),
        group: Some(Group {
            id: "g".into(),
            name: "g".into(),
            allowed_groups: vec!["g".into()],
        }),
    }
}
fn plane() -> DataPlane {
    let a = peer("a", [10, 77, 0, 2], 1);
    let b = peer("b", [10, 77, 0, 3], 2);
    let mut config = RoutingConfig::default();
    config.by_ip.insert(b.ip, "b".into());
    config.by_ip.insert(a.ip, "a".into());
    config.peers.insert("a".into(), a);
    config.peers.insert("b".into(), b);
    let mut dp = DataPlane::default();
    dp.config = config;
    dp
}
fn request() -> Vec<u8> {
    ipv4::test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false)
}
fn ingress(dp: &DataPlane, raw: &[u8]) -> IngressPacket {
    dp.ingress(&dp.authenticated_peer("a").unwrap(), raw)
        .unwrap()
}

#[test]
fn pending_is_bounded_expires_and_ttl_is_decremented_only_on_ingress() {
    let mut dp = plane();
    let raw = request();
    let now = Instant::now();
    for _ in 0..=PENDING_LIMIT {
        let prepared = dp.prepare(ingress(&dp, &raw), now).unwrap();
        assert_eq!(prepared.bytes()[8], 63);
        dp.finish(prepared, EgressOutcome::NotReady, now);
    }
    assert_eq!(
        dp.pending_counts(),
        (PENDING_LIMIT, PENDING_LIMIT * raw.len())
    );
    let mut remaining = dp.pending_len();
    let retry = dp
        .prepare_retry(&mut remaining, now + Duration::from_secs(1))
        .unwrap();
    assert_eq!(retry.bytes()[8], 63, "retry must not decrement TTL again");
    dp.finish(retry, EgressOutcome::NotReady, now + Duration::from_secs(1));
    let mut remaining = usize::MAX;
    assert!(dp
        .prepare_retry(&mut remaining, now + Duration::from_secs(4))
        .is_none());
    assert_eq!(dp.pending_counts(), (0, 0));
}

#[test]
fn stale_authenticated_identity_and_pending_route_provenance_are_rejected() {
    let mut dp = plane();
    let raw = request();
    let auth = dp.authenticated_peer("a").unwrap();
    let prepared = dp
        .prepare(dp.ingress(&auth, &raw).unwrap(), Instant::now())
        .unwrap();
    dp.finish(prepared, EgressOutcome::NotReady, Instant::now());
    let old = dp.config.clone();
    let mut new = old.clone();
    new.peers.insert("b".into(), peer("b", [10, 77, 0, 4], 2));
    new.by_ip.remove(&Ipv4Addr::new(10, 77, 0, 3));
    new.by_ip.insert(Ipv4Addr::new(10, 77, 0, 4), "b".into());
    dp.reconcile(&old, new);
    assert_eq!(dp.pending_counts(), (0, 0));
    assert!(
        dp.ingress(&auth, &raw).is_some(),
        "unchanged authenticated source remains valid"
    );
    let stale = dp.authenticated_peer("b").unwrap();
    let old_config = dp.config.clone();
    let mut changed = old_config.clone();
    changed
        .peers
        .insert("b".into(), peer("b", [10, 77, 0, 5], 9));
    dp.reconcile(&old_config, changed);
    assert!(dp.ingress(&stale, &raw).is_none());
}

#[test]
fn reply_only_retry_does_not_fall_back_after_mapping_expires() {
    for retry_at in [
        Duration::from_secs(60),
        Duration::from_secs(60) + Duration::from_millis(1),
    ] {
        let mut dp = plane();
        let now = Instant::now();
        let raw = request();
        let request = dp.prepare(ingress(&dp, &raw), now).unwrap();
        assert!(dp.finish(request, EgressOutcome::Delivered, now).is_some());
        let mut reply = ipv4::test_packet([10, 77, 0, 3], [10, 77, 0, 2], false, false);
        reply[20..22].copy_from_slice(&5678u16.to_be_bytes());
        reply[22..24].copy_from_slice(&1234u16.to_be_bytes());
        reply[10..12].fill(0);
        let sum = crate::kernel::checksum::checksum(&reply[..20]);
        reply[10..12].copy_from_slice(&sum.to_be_bytes());
        assert!(
            policy::route_allowed(
                dp.config.peers.get("b").unwrap().group.as_ref(),
                dp.config.peers.get("a").unwrap().group.as_ref()
            ),
            "reverse direct route is otherwise ACL-permitted"
        );
        let auth = dp.authenticated_peer("b").unwrap();
        let packet = dp.ingress(&auth, &reply).unwrap();
        let delivery = dp.prepare(packet, now).unwrap();
        assert_eq!(delivery.stamp.origin, RouteOrigin::ReplyOnly);
        dp.finish(
            delivery,
            EgressOutcome::NotReady,
            now + Duration::from_secs(59),
        );
        assert_eq!(dp.pending_counts(), (1, reply.len()));
        assert_eq!(
            dp.pending_counts(),
            (1, reply.len()),
            "queue deadline is later than flow expiry before retry"
        );
        let mut remaining = usize::MAX;
        assert!(
            dp.prepare_retry(&mut remaining, now + retry_at).is_none(),
            "expired reply mapping must not fall back to direct route"
        );
        // Flow expiration is reclaimed by the normal bounded sweep, not by
        // an unrelated reply lookup/retry.
        dp.expire(now + retry_at);
        assert_eq!(dp.flow_counts(), (0, 0), "retry creates no new direct flow");
        assert_eq!(dp.pending_counts(), (0, 0));
    }
}

#[test]
fn rewrite_failure_releases_new_reservation_and_preserves_active_flow() {
    let mut dp = plane();
    let now = Instant::now();
    let raw = request();
    let ingress = ingress(&dp, &raw);
    let source = dp.config.peers.get("a").unwrap().clone();
    let target = dp.config.peers.get("b").unwrap().clone();
    let (rewrite, reservation) = dp
        .flows
        .prepare_direct(&ingress.packet, &source, &target, now)
        .unwrap();
    assert!(rewrite_reserved(
        &mut dp.flows,
        &ingress.packet,
        rewrite,
        Some(reservation),
        now,
        |_, _| None
    )
    .is_none());
    assert_eq!(dp.flow_counts(), (0, 0));
    assert_eq!(dp.flows.test_index_counts(), (0, 0, 0));
    let (_, retry_reservation) = dp
        .flows
        .prepare_direct(&ingress.packet, &source, &target, now)
        .expect("same tuple can be prepared again");
    dp.flows.complete(retry_reservation, false, now);

    let (_rewrite, reservation) = dp
        .flows
        .prepare_direct(&ingress.packet, &source, &target, now)
        .unwrap();
    dp.flows.complete(reservation, true, now);
    let before = dp.flows.test_active_observation().unwrap().0;
    let (rewrite, reservation) = dp
        .flows
        .prepare_direct(
            &ingress.packet,
            &source,
            &target,
            now + Duration::from_secs(1),
        )
        .unwrap();
    assert!(rewrite_reserved(
        &mut dp.flows,
        &ingress.packet,
        rewrite,
        Some(reservation),
        now + Duration::from_secs(1),
        |_, _| None
    )
    .is_none());
    assert_eq!(dp.flow_counts(), (1, 0));
    assert_eq!(
        dp.flows.test_active_observation().unwrap().0,
        before,
        "failed rewrite does not refresh active flow"
    );
    assert_eq!(dp.flows.test_index_counts().0, 1);
}

#[test]
fn pending_deadline_is_not_extended_by_retries() {
    let mut dp = plane();
    let raw = request();
    let now = Instant::now();
    let prepared = dp.prepare(ingress(&dp, &raw), now).unwrap();
    dp.finish(prepared, EgressOutcome::NotReady, now);
    for secs in [1, 2] {
        let mut remaining = dp.pending_len();
        let retry = dp
            .prepare_retry(&mut remaining, now + Duration::from_secs(secs))
            .unwrap();
        dp.finish(
            retry,
            EgressOutcome::NotReady,
            now + Duration::from_secs(secs),
        );
    }
    assert_eq!(dp.pending_counts(), (1, raw.len()));
    let mut remaining = dp.pending_len();
    let retry = dp
        .prepare_retry(&mut remaining, now + Duration::from_millis(2999))
        .expect("delivery remains live until original deadline");
    dp.finish(
        retry,
        EgressOutcome::NotReady,
        now + Duration::from_millis(2999),
    );
    assert_eq!(dp.pending_counts(), (1, raw.len()));
    let mut remaining = dp.pending_len();
    assert!(dp
        .prepare_retry(&mut remaining, now + Duration::from_secs(3))
        .is_none());
    assert_eq!(dp.pending_counts(), (0, 0));
}

#[test]
fn pending_count_and_byte_limits_are_independent() {
    let mut count_plane = plane();
    let now = Instant::now();
    for _ in 0..=PENDING_LIMIT {
        let mut raw = request();
        raw[9] = 1;
        raw[20] = 8;
        raw[21] = 0;
        raw[22..24].fill(0);
        let icmp_sum = crate::kernel::checksum::checksum(&raw[20..]);
        raw[22..24].copy_from_slice(&icmp_sum.to_be_bytes());
        raw[10..12].fill(0);
        let ip_sum = crate::kernel::checksum::checksum(&raw[..20]);
        raw[10..12].copy_from_slice(&ip_sum.to_be_bytes());
        let ingress = ingress(&count_plane, &raw);
        let prepared = count_plane.prepare(ingress, now).unwrap();
        count_plane.finish(prepared, EgressOutcome::NotReady, now);
    }
    assert_eq!(
        count_plane.pending_counts(),
        (PENDING_LIMIT, PENDING_LIMIT * 28)
    );

    let mut byte_plane = plane();
    let mut accepted_bytes = 0;
    let mut accepted = 0;
    for port in 0..32u16 {
        let mut raw = vec![0; 65535];
        raw[0] = 0x45;
        raw[2..4].copy_from_slice(&65535u16.to_be_bytes());
        raw[8] = 64;
        raw[9] = 17;
        raw[12..16].copy_from_slice(&[10, 77, 0, 2]);
        raw[16..20].copy_from_slice(&[10, 77, 0, 3]);
        raw[20..22].copy_from_slice(&port.to_be_bytes());
        raw[22..24].copy_from_slice(&5678u16.to_be_bytes());
        raw[24..26].copy_from_slice(&65515u16.to_be_bytes());
        let sum = crate::kernel::checksum::checksum(&raw[..20]);
        raw[10..12].copy_from_slice(&sum.to_be_bytes());
        let ingress = ingress(&byte_plane, &raw);
        let prepared = byte_plane.prepare(ingress, now).unwrap();
        byte_plane.finish(prepared, EgressOutcome::NotReady, now);
        if byte_plane.pending_len() == accepted + 1 {
            accepted += 1;
            accepted_bytes += raw.len();
        }
    }
    assert!(accepted < PENDING_LIMIT && accepted_bytes <= PENDING_BYTES);
    assert!(
        accepted_bytes + 65535 > PENDING_BYTES,
        "byte limit, not count limit, rejected the next packet"
    );
    assert_eq!(byte_plane.pending_counts(), (accepted, accepted_bytes));
    assert_eq!(
        byte_plane.flow_counts(),
        (0, 0),
        "NotReady rolls back all active/pending flow and quota state"
    );
}

#[test]
fn retry_batch_budget_counts_discarded_items_and_delivers_following_live_item() {
    let mut dp = plane();
    let now = Instant::now();
    let raw = request();
    let valid_stamp = RouteStamp {
        source_id: "a".into(),
        source_key: dp.config.peers["a"].key_identity(),
        source_ip: dp.config.peers["a"].ip,
        target_id: "b".into(),
        target_key: dp.config.peers["b"].key_identity(),
        target_ip: dp.config.peers["b"].ip,
        origin: RouteOrigin::Direct,
    };
    let live = ingress(&dp, &raw);
    let live_delivery = dp.prepare(live.clone(), now).unwrap();
    dp.finish(live_delivery, EgressOutcome::NotReady, now);

    // Build three independent discard cases ahead of the valid queue item.
    // They are observations only: none owns a flow reservation.
    let expired = PendingPacket {
        ingress: live.clone(),
        stamp: valid_stamp.clone(),
        deadline: now,
    };
    let mut bad_stamp = valid_stamp.clone();
    bad_stamp.target_ip = Ipv4Addr::new(10, 77, 0, 99);
    let bad_provenance = PendingPacket {
        ingress: live.clone(),
        stamp: bad_stamp,
        deadline: now + PENDING_TTL,
    };
    let icmp_raw = ipv4::test_icmp_error(
        Ipv4Addr::new(10, 77, 0, 2),
        Ipv4Addr::new(10, 77, 0, 3),
        3,
        3,
        &raw,
    );
    let icmp_ingress = dp
        .ingress(&dp.authenticated_peer("a").unwrap(), &icmp_raw)
        .unwrap();
    let cannot_prepare = PendingPacket {
        ingress: icmp_ingress,
        stamp: valid_stamp,
        deadline: now + PENDING_TTL,
    };
    for item in [expired, bad_provenance, cannot_prepare].into_iter().rev() {
        dp.pending.bytes += item.ingress.packet.bytes().len();
        dp.pending.entries.push_front(item);
    }

    let mut remaining = 4;
    let delivery = dp
        .prepare_retry(&mut remaining, now + Duration::from_millis(1))
        .expect("the live item must be prepared in the same batch after three discards");
    assert_eq!(remaining, 0, "each popped entry consumes budget");
    assert_eq!(dp.pending_len(), 0);
    assert_eq!(
        delivery.bytes()[8],
        63,
        "retry does not decrement TTL a second time"
    );
    dp.finish(
        delivery,
        EgressOutcome::NotReady,
        now + Duration::from_millis(1),
    );
    assert_eq!(
        dp.pending_counts(),
        (1, raw.len()),
        "NotReady requeues one original copy only"
    );
    assert_eq!(
        dp.flow_counts(),
        (0, 0),
        "retry does not retain flow reservations"
    );
}

#[test]
fn established_tcp_related_icmp_queue_is_cleared_when_initiator_acl_is_revoked() {
    fn tcp(
        src: [u8; 4],
        dst: [u8; 4],
        sport: u16,
        dport: u16,
        flags: u8,
        seq: u32,
        ack: u32,
    ) -> Vec<u8> {
        let mut raw = vec![0; 40];
        raw[0] = 0x45;
        raw[2..4].copy_from_slice(&40u16.to_be_bytes());
        raw[8] = 64;
        raw[9] = 6;
        raw[12..16].copy_from_slice(&src);
        raw[16..20].copy_from_slice(&dst);
        raw[20..22].copy_from_slice(&sport.to_be_bytes());
        raw[22..24].copy_from_slice(&dport.to_be_bytes());
        raw[24..28].copy_from_slice(&seq.to_be_bytes());
        raw[28..32].copy_from_slice(&ack.to_be_bytes());
        raw[32] = 0x50;
        raw[33] = flags;
        let mut pseudo = Vec::new();
        pseudo.extend_from_slice(&src);
        pseudo.extend_from_slice(&dst);
        pseudo.extend_from_slice(&[0, 6]);
        pseudo.extend_from_slice(&20u16.to_be_bytes());
        pseudo.extend_from_slice(&raw[20..]);
        let sum = crate::kernel::checksum::checksum(&pseudo);
        raw[36..38].copy_from_slice(&sum.to_be_bytes());
        let sum = crate::kernel::checksum::checksum(&raw[..20]);
        raw[10..12].copy_from_slice(&sum.to_be_bytes());
        raw
    }
    let mut dp = plane();
    let t0 = Instant::now();
    let a = [10, 77, 0, 2];
    let b = [10, 77, 0, 3];
    let syn = tcp(a, b, 1234, 443, 0x02, 100, 0);
    let syn_delivery = dp.prepare(ingress(&dp, &syn), t0).unwrap();
    dp.finish(syn_delivery, EgressOutcome::Delivered, t0);
    let syn_ack = tcp(b, a, 443, 1234, 0x12, 200, 101);
    let syn_ack_ingress = dp
        .ingress(&dp.authenticated_peer("b").unwrap(), &syn_ack)
        .unwrap();
    let syn_ack_delivery = dp
        .prepare(syn_ack_ingress, t0 + Duration::from_secs(1))
        .unwrap();
    dp.finish(
        syn_ack_delivery,
        EgressOutcome::Delivered,
        t0 + Duration::from_secs(1),
    );
    let ack = tcp(a, b, 1234, 443, 0x10, 101, 201);
    let ack_delivery = dp
        .prepare(ingress(&dp, &ack), t0 + Duration::from_secs(2))
        .unwrap();
    dp.finish(
        ack_delivery,
        EgressOutcome::Delivered,
        t0 + Duration::from_secs(2),
    );
    assert_eq!(dp.flow_counts(), (1, 0));
    let deadline = dp.flows.test_active_observation().unwrap().0;

    // An unrelated topology addition must not invalidate the established mapping.
    let old = dp.config.clone();
    let mut unchanged = old.clone();
    unchanged
        .peers
        .insert("c".into(), peer("c", [10, 77, 0, 4], 3));
    dp.reconcile(&old, unchanged);
    assert_eq!(dp.flow_counts(), (1, 0));

    let icmp = ipv4::test_icmp_error(Ipv4Addr::from(b), Ipv4Addr::from(a), 3, 3, &syn);
    let related = dp
        .ingress(&dp.authenticated_peer("b").unwrap(), &icmp)
        .unwrap();
    let related_delivery = dp
        .prepare(related.clone(), t0 + Duration::from_secs(59))
        .expect("committed TCP mapping admits related ICMP");
    assert_eq!(related_delivery.stamp.origin, RouteOrigin::ReplyOnly);
    assert_eq!(
        related_delivery.bytes()[8],
        63,
        "ICMP TTL is decremented once on ingress"
    );
    dp.finish(
        related_delivery,
        EgressOutcome::NotReady,
        t0 + Duration::from_secs(59),
    );
    assert_eq!(dp.pending_counts(), (1, icmp.len()));
    assert_eq!(
        dp.flows.test_active_observation().unwrap().0,
        deadline,
        "related ICMP does not refresh the established flow deadline"
    );

    let old = dp.config.clone();
    let mut revoked = old.clone();
    let source = revoked.peers.get_mut("a").unwrap();
    source.group = Some(Group {
        id: "g".into(),
        name: "g".into(),
        allowed_groups: vec![],
    });
    dp.reconcile(&old, revoked);
    assert_eq!(
        dp.flow_counts(),
        (0, 0),
        "ACL revocation clears active and pending flow state"
    );
    assert_eq!(
        dp.flows.test_index_counts(),
        (0, 0, 0),
        "ACL revocation clears reverse indexes and quota counts"
    );
    assert_eq!(
        dp.pending_counts(),
        (0, 0),
        "ACL revocation clears queued bytes"
    );
    assert!(
        dp.prepare(related, t0 + Duration::from_secs(60)).is_none(),
        "related ICMP never falls back to the otherwise permitted reverse direct route"
    );
    let mut remaining = 1;
    assert!(
        dp.prepare_retry(&mut remaining, t0 + Duration::from_secs(60))
            .is_none(),
        "expired mapping cannot fall back to a reverse direct route after revoke"
    );
    assert_eq!(remaining, 1);
}

#[test]
fn stateless_icmp_echo_delivery_needs_acl_and_never_creates_flow() {
    let mut dp = plane();
    let now = Instant::now();
    let mut raw = vec![0; 28];
    raw[0] = 0x45;
    raw[2..4].copy_from_slice(&28u16.to_be_bytes());
    raw[8] = 64;
    raw[9] = 1;
    raw[12..16].copy_from_slice(&[10, 77, 0, 2]);
    raw[16..20].copy_from_slice(&[10, 77, 0, 3]);
    raw[20] = 8;
    let sum = crate::kernel::checksum::checksum(&raw[20..]);
    raw[22..24].copy_from_slice(&sum.to_be_bytes());
    let sum = crate::kernel::checksum::checksum(&raw[..20]);
    raw[10..12].copy_from_slice(&sum.to_be_bytes());
    let ingress = ingress(&dp, &raw);
    let delivery = dp
        .prepare(ingress.clone(), now)
        .expect("group ACL permits echo");
    assert!(delivery.reservation.is_none());
    dp.finish(delivery, EgressOutcome::Delivered, now);
    assert_eq!(dp.flow_counts(), (0, 0));
    let old = dp.config.clone();
    let mut new = old.clone();
    new.peers.get_mut("b").unwrap().group = Some(Group {
        id: "other".into(),
        name: "other".into(),
        allowed_groups: vec![],
    });
    new.peers.get_mut("b").unwrap().group_id = "other".into();
    dp.reconcile(&old, new);
    assert!(
        dp.prepare(ingress, now + Duration::from_secs(1)).is_none(),
        "revoked ACL denies stateless echo"
    );
    assert_eq!(dp.flow_counts(), (0, 0));
}

#[test]
fn forward_pending_provenance_changes_are_reconciled_without_retry_or_reservation() {
    for changed in [
        "source ip",
        "target ip",
        "source key",
        "target key",
        "forward id",
        "protocol",
        "port",
        "target",
        "allowlist",
        "acl",
    ] {
        let mut dp = plane();
        let now = Instant::now();
        let hub = Ipv4Addr::new(10, 77, 0, 1);
        dp.config.hub_ip = Some(hub);
        dp.config.forwards = vec![ForwardConfig {
            id: "f".into(),
            protocol: TransportProtocol::Udp,
            target_peer_id: "b".into(),
            target_port: 5678,
            allowed_group_ids: vec!["g".into()],
        }];
        let mut raw = ipv4::test_packet([10, 77, 0, 2], hub.octets(), false, false);
        raw[22..24].copy_from_slice(&5678u16.to_be_bytes());
        raw[10..12].fill(0);
        let sum = crate::kernel::checksum::checksum(&raw[..20]);
        raw[10..12].copy_from_slice(&sum.to_be_bytes());
        let p = ingress(&dp, &raw);
        let prepared = dp.prepare(p, now).unwrap();
        dp.finish(prepared, EgressOutcome::NotReady, now);
        assert_eq!(
            dp.pending_counts().0,
            1,
            "fixture must hold forward pending for {changed}"
        );
        let old = dp.config.clone();
        let mut new = old.clone();
        match changed {
            "source ip" => {
                new.peers.get_mut("a").unwrap().ip = Ipv4Addr::new(10, 77, 0, 9);
                new.by_ip.remove(&Ipv4Addr::new(10, 77, 0, 2));
                new.by_ip.insert(Ipv4Addr::new(10, 77, 0, 9), "a".into());
            }
            "target ip" => {
                new.peers.get_mut("b").unwrap().ip = Ipv4Addr::new(10, 77, 0, 9);
                new.by_ip.remove(&Ipv4Addr::new(10, 77, 0, 3));
                new.by_ip.insert(Ipv4Addr::new(10, 77, 0, 9), "b".into());
            }
            "source key" => new.peers.get_mut("a").unwrap().key = [9; 32],
            "target key" => new.peers.get_mut("b").unwrap().key = [9; 32],
            "acl" => {
                new.peers.get_mut("a").unwrap().group = Some(Group {
                    id: "g2".into(),
                    name: "g2".into(),
                    allowed_groups: vec![],
                });
                new.peers.get_mut("a").unwrap().group_id = "g2".into();
            }
            "forward id" => new.forwards[0].id = "changed".into(),
            "protocol" => new.forwards[0].protocol = TransportProtocol::Tcp,
            "port" => new.forwards[0].target_port = 5679,
            "target" => new.forwards[0].target_peer_id = "a".into(),
            "allowlist" => new.forwards[0].allowed_group_ids.clear(),
            _ => unreachable!(),
        }
        dp.reconcile(&old, new);
        assert_eq!(
            dp.pending_counts(),
            (0, 0),
            "changed {changed} invalidates queued provenance"
        );
        let mut remaining = 1;
        assert!(
            dp.prepare_retry(&mut remaining, now + Duration::from_millis(1))
                .is_none(),
            "no stale item is retried after {changed}"
        );
        assert_eq!(
            dp.flow_counts(),
            (0, 0),
            "reconcile leaves no reservation after {changed}"
        );
    }
}

#[test]
fn pending_deadline_is_preserved_at_2999ms_and_expires_at_three_seconds() {
    let mut dp = plane();
    let now = Instant::now();
    let raw = request();
    let prepared = dp.prepare(ingress(&dp, &raw), now).unwrap();
    dp.finish(prepared, EgressOutcome::NotReady, now);
    for at in [
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_millis(2999),
    ] {
        let mut remaining = dp.pending_len();
        let retry = dp
            .prepare_retry(&mut remaining, now + at)
            .expect("must remain live before original deadline");
        dp.finish(retry, EgressOutcome::NotReady, now + at);
    }
    assert_eq!(dp.pending_counts(), (1, raw.len()));
    let mut remaining = dp.pending_len();
    assert!(dp
        .prepare_retry(&mut remaining, now + Duration::from_secs(3))
        .is_none());
    assert_eq!(dp.pending_counts(), (0, 0));
}
