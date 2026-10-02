use super::*;
use crate::kernel::{Kernel, KernelHandle, ReloadError, RunError, SnapshotLoadError, StartError};
use crate::model::Forward;
use crate::storage::Store;
async fn initialize_store_runtime(
    socket: UdpSocket,
    store: Arc<Store>,
    key: [u8; 32],
) -> Result<(Kernel, KernelHandle), StartError> {
    Kernel::initialize(socket, key, move || {
        let raw = store.runtime_snapshot().map_err(|_| SnapshotLoadError)?;
        crate::kernel::snapshot::CompiledSnapshot::try_from(raw).map_err(|_| SnapshotLoadError)
    })
    .await
}
use crate::kernel::checksum::checksum;
use crate::model::Group;
use base64::Engine;
use boringtun::noise::rate_limiter::RateLimiter;
use boringtun::x25519::StaticSecret;
use tokio::time::timeout;
fn create_forward_for_test(store: &Store, mut forward: Forward) {
    store.create_forward(&mut forward).unwrap();
}

fn empty_compiled() -> crate::kernel::snapshot::CompiledSnapshot {
    crate::kernel::snapshot::CompiledSnapshot::try_from(crate::model::NetworkSnapshot {
        settings: None,
        groups: vec![],
        peers: vec![],
        forwards: vec![],
    })
    .unwrap()
}

#[tokio::test]
async fn initialization_rejects_invalid_snapshot() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let result = Kernel::initialize(socket, [1; 32], || Err(SnapshotLoadError)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn dropping_unpolled_run_future_clears_readiness() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (kernel, handle) = Kernel::initialize(socket, [2; 32], || Ok(empty_compiled()))
        .await
        .unwrap();
    assert!(handle.is_ready());
    drop(kernel.run());
    assert!(!handle.is_ready());
}

#[tokio::test]
async fn dropping_initialized_kernel_clears_readiness() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (kernel, handle) = Kernel::initialize(socket, [22; 32], || Ok(empty_compiled()))
        .await
        .unwrap();
    assert!(handle.is_ready());
    drop(kernel);
    assert!(!handle.is_ready());
}

#[tokio::test]
async fn aborted_run_task_clears_readiness_and_stopped_exit_is_not_ready() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (kernel, handle) = Kernel::initialize(socket, [23; 32], || Ok(empty_compiled()))
        .await
        .unwrap();
    let task = tokio::spawn(kernel.run());
    tokio::task::yield_now().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!handle.is_ready());

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (mut kernel, handle) = Kernel::initialize(socket, [24; 32], || Ok(empty_compiled()))
        .await
        .unwrap();
    kernel.commands.close();
    let task = tokio::spawn(kernel.run());
    assert!(matches!(task.await.unwrap(), Err(RunError::Stopped)));
    assert!(!handle.is_ready());
}

#[tokio::test]
async fn kernel_panic_clears_readiness_and_queued_reload_survives_caller_cancellation() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let load_calls = calls.clone();
    let (kernel, handle) = Kernel::initialize(socket, [3; 32], move || {
        if load_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            panic!("loader panic");
        }
        Ok(empty_compiled())
    })
    .await
    .unwrap();
    let task = tokio::spawn(kernel.run());
    let reload = tokio::spawn({
        let handle = handle.clone();
        async move { handle.reload().await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while calls.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    reload.abort();
    let joined = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap();
    assert!(joined.unwrap_err().is_panic());
    assert!(
        !handle.is_ready(),
        "guard drop after loader panic clears readiness"
    );
    assert_eq!(handle.reload().await, Err(ReloadError::Stopped));
}

#[tokio::test]
async fn canceled_reload_ack_wait_does_not_cancel_queued_command() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (kernel, handle) = Kernel::initialize(socket, [25; 32], {
        let calls = calls.clone();
        move || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(empty_compiled())
        }
    })
    .await
    .unwrap();
    let caller = tokio::spawn({
        let handle = handle.clone();
        async move { handle.reload().await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while handle.commands.capacity() == 16 {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    caller.abort();
    let task = tokio::spawn(kernel.run());
    tokio::time::timeout(Duration::from_secs(1), async {
        while calls.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    assert!(handle.is_ready());
    task.abort();
    let _ = task.await;
}

#[test]
fn persisted_peer_address_validation_accepts_last_peer_ip_and_rejects_reserved_addresses() {
    for (ip, expected) in [
        ("10.77.0.254", true),
        ("10.77.0.1", false),
        ("10.77.0.255", false),
    ] {
        let store = Store::open(":memory:").unwrap();
        store.bind_test_identity();
        store
            .setup("10.77.0.0/24", "hub.example:51820", 25)
            .unwrap();
        store
            .add_group(&Group {
                id: "g".into(),
                name: "g".into(),
                allowed_groups: vec![],
            })
            .unwrap();
        let secret = StaticSecret::from([211u8; 32]);
        store
            .add_peer(&Peer {
                id: "p".into(),
                name: "p".into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(&secret).as_bytes()),
                ipv4: ip.into(),
                group_id: "g".into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
        let snapshot = store.runtime_snapshot().unwrap();
        assert_eq!(
            crate::kernel::snapshot::CompiledSnapshot::try_from(snapshot).is_ok(),
            expected,
            "{ip}"
        );
    }
}

#[test]
fn wireguard_reconcile_retains_id_and_key_tunnel_but_updates_identity_ip() {
    let mut wg = wireguard::WireGuard::new([213; 32]);
    let key = [7; 32];
    let retained = wg
        .install(vec![wireguard::PeerIdentity {
            id: "p".into(),
            key,
            ip: "10.77.0.2".parse().unwrap(),
        }])
        .unwrap();
    assert_eq!(retained.get("p"), Some(&false));
    let index = wg.receiver_index("p").unwrap();
    let endpoint = "127.0.0.1:5000".parse().unwrap();
    wg.set_endpoint("p", Some(endpoint));
    let retained = wg
        .install(vec![wireguard::PeerIdentity {
            id: "p".into(),
            key,
            ip: "10.77.0.3".parse().unwrap(),
        }])
        .unwrap();
    assert_eq!(retained.get("p"), Some(&true));
    assert_eq!(wg.receiver_index("p"), Some(index));
    assert_eq!(wg.endpoint("p"), Some(endpoint));
    assert_eq!(
        wg.identity("p").unwrap().ip,
        "10.77.0.3".parse::<Ipv4Addr>().unwrap()
    );
    let replaced = wg
        .install(vec![wireguard::PeerIdentity {
            id: "p".into(),
            key: [8; 32],
            ip: "10.77.0.3".parse().unwrap(),
        }])
        .unwrap();
    assert_eq!(replaced.get("p"), Some(&false));
    assert_ne!(wg.receiver_index("p"), Some(index));
}

#[tokio::test]
async fn apply_snapshot_seeds_persisted_stats_and_retained_reload_keeps_runtime_stats() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("persisted-stats.sqlite");
    let store = Arc::new(Store::open(db.to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("10.77.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "g".into(),
            name: "g".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let secret = StaticSecret::from([214u8; 32]);
    store
        .add_peer(&Peer {
            id: "p".into(),
            name: "p".into(),
            public_key: base64::engine::general_purpose::STANDARD
                .encode(PublicKey::from(&secret).as_bytes()),
            ipv4: "10.77.0.2".into(),
            group_id: "g".into(),
            received_bytes: 111,
            sent_bytes: 222,
            last_handshake_unix: Some(333),
        })
        .unwrap();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE peers SET rx=111,tx=222,last_handshake=333 WHERE id='p'",
            [],
        )
        .unwrap();
    let load = || {
        crate::kernel::snapshot::CompiledSnapshot::try_from(store.runtime_snapshot().unwrap())
            .unwrap()
    };
    let mut state = RuntimeState {
        wireguard: wireguard::WireGuard::new([215; 32]),
        dataplane: crate::kernel::dataplane::DataPlane::default(),
        stats: HashMap::new(),
    };
    install_snapshot(load(), &mut state).unwrap();
    assert_eq!(
        state.stats["p"],
        PeerRuntimeStats {
            rx_bytes: 111,
            tx_bytes: 222,
            last_handshake_unix: Some(333),
            last_data_unix: None
        }
    );
    state.stats.get_mut("p").unwrap().rx_bytes = 444;
    state.stats.get_mut("p").unwrap().tx_bytes = 555;
    state.stats.get_mut("p").unwrap().last_handshake_unix = Some(666);
    state.stats.get_mut("p").unwrap().last_data_unix = Some(777);
    install_snapshot(load(), &mut state).unwrap();
    assert_eq!(
        state.stats["p"],
        PeerRuntimeStats {
            rx_bytes: 444,
            tx_bytes: 555,
            last_handshake_unix: Some(666),
            last_data_unix: Some(777)
        }
    );
}

#[tokio::test]
async fn wireguard_deliver_cold_known_endpoint_only_emits_handshake() {
    let mut wg = wireguard::WireGuard::new([216; 32]);
    wg.install(vec![wireguard::PeerIdentity {
        id: "p".into(),
        key: *PublicKey::from(&StaticSecret::from([217; 32])).as_bytes(),
        ip: "10.77.0.2".parse().unwrap(),
    }])
    .unwrap();
    let sink = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let endpoint = sink.local_addr().unwrap();
    wg.set_endpoint("p", Some(endpoint));
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut out = vec![0; 65535];
    let plaintext = ipv4::test_packet([10, 77, 0, 1], [10, 77, 0, 2], false, false);
    assert_eq!(
        wg.deliver(&sender, "p", &plaintext, &mut out).await,
        wireguard::TransportOutcome::NotReady
    );
    let (n, _) = timeout(Duration::from_secs(1), sink.recv_from(&mut out))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(out[..4].try_into().unwrap()),
        1,
        "only a real handshake initiation reaches the endpoint"
    );
    assert_ne!(&out[..n], plaintext.as_slice());
}

#[test]
fn snapshot_retry_schedule_caps_and_resets_after_success() {
    let start = Instant::now();
    let mut retry = SnapshotRetry::new();
    let mut at = start;
    for (next_delay, elapsed) in [
        (2, 1),
        (4, 3),
        (8, 7),
        (16, 15),
        (30, 31),
        (30, 61),
        (30, 91),
    ] {
        at = retry.failed_at(at);
        assert_eq!(at.duration_since(start), Duration::from_secs(elapsed));
        assert_eq!(retry.delay, Duration::from_secs(next_delay));
    }
    retry.succeeded();
    assert_eq!(retry.failed_at(start).duration_since(start), RETRY_INITIAL);
}

#[test]
fn udp_receive_error_classifier_recovers_expected_datagram_errors_only() {
    for kind in [
        std::io::ErrorKind::Interrupted,
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::ConnectionRefused,
        std::io::ErrorKind::AddrNotAvailable,
    ] {
        assert!(recoverable_udp_error(kind), "{kind:?}");
    }
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::NotConnected,
        std::io::ErrorKind::Other,
    ] {
        assert!(!recoverable_udp_error(kind), "{kind:?}");
    }
}

#[tokio::test]
async fn udp_receive_task_survives_recoverable_errors_and_fails_on_fatal_error() {
    async fn injected_receiver(
        mut incoming: mpsc::UnboundedReceiver<std::io::Result<u8>>,
        readiness: Arc<AtomicBool>,
    ) -> Result<u8, std::io::Error> {
        loop {
            match classify_udp_receive(
                incoming.recv().await.expect("injected receive result"),
                readiness.as_ref(),
            )? {
                Some(datagram) => return Ok(datagram),
                None => continue,
            }
        }
    }
    let readiness = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(injected_receiver(rx, readiness.clone()));
    for kind in [
        std::io::ErrorKind::Interrupted,
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::ConnectionRefused,
    ] {
        tx.send(Err(std::io::Error::from(kind))).unwrap();
    }
    tx.send(Ok(42)).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        42
    );
    assert!(
        readiness.load(Ordering::Acquire),
        "recoverable receive errors do not degrade readiness"
    );

    let fatal_readiness = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(injected_receiver(rx, fatal_readiness.clone()));
    tx.send(Err(std::io::Error::from(
        std::io::ErrorKind::PermissionDenied,
    )))
    .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert!(
        !fatal_readiness.load(Ordering::Acquire),
        "fatal receive error clears readiness before task exit"
    );
}

#[test]
fn forward_requires_both_allowlist_and_directed_backend_acl() {
    let forward = Forward {
        id: "f".into(),
        name: "f".into(),
        protocol: "tcp".into(),
        target_peer_id: "b".into(),
        target_port: 8080,
        allowed_group_ids: vec!["a".into()],
    };
    let group = Group {
        id: "a".into(),
        name: "a".into(),
        allowed_groups: vec!["b".into()],
    };
    assert!(policy::forward_allowed(
        &forward.clone().into(),
        "a",
        Some(&group),
        "b"
    ));
    assert!(!policy::forward_allowed(
        &forward.clone().into(),
        "c",
        Some(&group),
        "b"
    ));
    let denied = Group {
        allowed_groups: vec![],
        ..group
    };
    assert!(!policy::forward_allowed(
        &forward.into(),
        "a",
        Some(&denied),
        "b"
    ));
}
#[tokio::test]
async fn forward_load_errors_fail_closed_and_are_acknowledged() {
    async fn broken_store(path: &std::path::Path) -> Arc<Store> {
        let store = Arc::new(Store::open(path.to_str().unwrap()).unwrap());
        store.bind_test_identity();
        store
            .setup("10.77.0.0/24", "hub.example:51820", 25)
            .unwrap();
        store
            .add_group(&Group {
                id: "g".into(),
                name: "g".into(),
                allowed_groups: vec![],
            })
            .unwrap();
        let secret = StaticSecret::from([41u8; 32]);
        store
            .add_peer(&Peer {
                id: "p".into(),
                name: "p".into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(&secret).as_bytes()),
                ipv4: "10.77.0.2".into(),
                group_id: "g".into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
        create_forward_for_test(
            &store,
            Forward {
                id: "f".into(),
                name: "f".into(),
                protocol: "udp".into(),
                target_peer_id: "p".into(),
                target_port: 53,
                allowed_group_ids: vec!["g".into()],
            },
        );
        store
    }
    let dir = tempfile::tempdir().unwrap();
    let startup_store = broken_store(&dir.path().join("startup.sqlite")).await;
    rusqlite::Connection::open(dir.path().join("startup.sqlite"))
        .unwrap()
        .execute("UPDATE forwards SET allowed='not-json'", [])
        .unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let result = initialize_store_runtime(socket, startup_store, [42; 32]).await;
    assert!(result.is_err(), "invalid startup snapshot is rejected");

    let reload_store = broken_store(&dir.path().join("reload.sqlite")).await;
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (kernel, handle) = initialize_store_runtime(socket, reload_store.clone(), [43; 32])
        .await
        .unwrap();
    let task = tokio::spawn(kernel.run());
    assert!(handle.is_ready());
    rusqlite::Connection::open(dir.path().join("reload.sqlite"))
        .unwrap()
        .execute("UPDATE forwards SET allowed='not-json'", [])
        .unwrap();
    assert_eq!(
        handle.reload().await,
        Err(ReloadError::Rejected),
        "reload reports invalid persisted forward state"
    );
    assert!(!handle.is_ready(), "failed reload marks runtime degraded");
    assert!(
        handle.stats().await.is_empty(),
        "failed reload clears runtime peer stats"
    );
    time::sleep(Duration::from_millis(1200)).await;
    assert!(!handle.is_ready(), "failed retry remains fail-closed");
    rusqlite::Connection::open(dir.path().join("reload.sqlite"))
        .unwrap()
        .execute("UPDATE forwards SET allowed='[\"g\"]'", [])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while !handle.is_ready() && Instant::now() < deadline {
        time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        handle.is_ready(),
        "timer recovers after a complete valid snapshot is restored"
    );
    assert!(
        handle.stats().await.contains_key("p"),
        "recovery installs the latest persisted peer snapshot"
    );
    task.abort();
}

#[tokio::test]
async fn malformed_snapshot_recovery_installs_only_latest_policy_and_peers() {
    timeout(Duration::from_secs(20), async {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("latest-policy.sqlite");
        let store = Arc::new(Store::open(db.to_str().unwrap()).unwrap());
        store.bind_test_identity();
        store
            .setup("10.88.0.0/24", "hub.example:51820", 25)
            .unwrap();
        for group in [
            Group {
                id: "a".into(),
                name: "A".into(),
                allowed_groups: vec!["b".into()],
            },
            Group {
                id: "c".into(),
                name: "C".into(),
                allowed_groups: vec!["b".into()],
            },
            Group {
                id: "b".into(),
                name: "B".into(),
                allowed_groups: vec![],
            },
        ] {
            store.add_group(&group).unwrap();
        }
        store.set_acl("a", &["b".into()]).unwrap();
        store.set_acl("c", &["b".into()]).unwrap();
        let secrets = [
            StaticSecret::from([81u8; 32]),
            StaticSecret::from([82u8; 32]),
            StaticSecret::from([83u8; 32]),
        ];
        for ((id, ip, group), secret) in [
            ("a", "10.88.0.2", "a"),
            ("b", "10.88.0.3", "b"),
            ("c", "10.88.0.4", "c"),
        ]
        .into_iter()
        .zip(secrets.iter())
        {
            store
                .add_peer(&Peer {
                    id: id.into(),
                    name: id.into(),
                    public_key: base64::engine::general_purpose::STANDARD
                        .encode(PublicKey::from(secret).as_bytes()),
                    ipv4: ip.into(),
                    group_id: group.into(),
                    received_bytes: 0,
                    sent_bytes: 0,
                    last_handshake_unix: None,
                })
                .unwrap();
        }
        let hub_private = [84u8; 32];
        let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
            .await
            .unwrap();
        let task = tokio::spawn(kernel.run());
        assert!(handle.is_ready());
        let sockets = [
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
        ];
        let mut clients: Vec<Tunn> = secrets
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, s)| Tunn::new(s, hub_public, None, None, 90 + i as u32, None))
            .collect();
        let mut tx = vec![0; 65535];
        let mut rx = vec![0; 65535];
        for i in 0..3 {
            establish_client(&sockets[i], address, &mut clients[i], &mut tx, &mut rx).await;
        }
        let packet = |src, dst, payload| service_packet(17, src, dst, 12000, 9000, 0, payload);
        for (index, source, dest, payload) in [
            (0, [10, 88, 0, 2], [10, 88, 0, 3], b"initial-a".as_slice()),
            (2, [10, 88, 0, 4], [10, 88, 0, 3], b"initial-c".as_slice()),
        ] {
            send_inner(
                &sockets[index],
                address,
                &mut clients[index],
                &packet(source, dest, payload),
                &mut tx,
            )
            .await;
            assert_eq!(
                &timeout(
                    Duration::from_secs(2),
                    recv_inner(&sockets[1], &mut clients[1], &mut rx, &mut tx)
                )
                .await
                .unwrap()[28..],
                payload
            );
        }

        rusqlite::Connection::open(&db)
            .unwrap()
            .execute("UPDATE groups SET allowed='broken-json' WHERE id='c'", [])
            .unwrap();
        assert_eq!(handle.reload().await, Err(ReloadError::Rejected));
        assert!(!handle.is_ready());
        assert!(
            handle.stats().await.is_empty(),
            "failed snapshot publishes no partial peer statistics"
        );
        // Mutate the still-malformed database while fail-closed: revoke A,
        // and add a new group/peer. Timer retries must not expose a partial install.
        store.set_acl("a", &[]).unwrap();
        store
            .add_group(&Group {
                id: "new".into(),
                name: "New".into(),
                allowed_groups: vec![],
            })
            .unwrap();
        let d_secret = StaticSecret::from([85u8; 32]);
        store
            .add_peer(&Peer {
                id: "d".into(),
                name: "D".into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(&d_secret).as_bytes()),
                ipv4: "10.88.0.5".into(),
                group_id: "new".into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
        time::sleep(Duration::from_millis(2200)).await;
        assert!(!handle.is_ready());
        assert!(
            handle.stats().await.is_empty(),
            "retry failure remains empty and fail-closed"
        );

        rusqlite::Connection::open(&db)
            .unwrap()
            .execute("UPDATE groups SET allowed='[\"b\"]' WHERE id='c'", [])
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while !handle.is_ready() && Instant::now() < deadline {
            time::sleep(Duration::from_millis(40)).await;
        }
        assert!(
            handle.is_ready(),
            "timer retries the repaired latest snapshot without another reload"
        );
        assert!(
            handle.stats().await.contains_key("d"),
            "latest complete snapshot includes the added peer"
        );
        clients[0] = Tunn::new(secrets[0].clone(), hub_public, None, None, 90, None);
        clients[1] = Tunn::new(secrets[1].clone(), hub_public, None, None, 91, None);
        clients[2] = Tunn::new(secrets[2].clone(), hub_public, None, None, 92, None);
        for i in [0, 1, 2] {
            establish_client(&sockets[i], address, &mut clients[i], &mut tx, &mut rx).await;
        }
        send_inner(
            &sockets[0],
            address,
            &mut clients[0],
            &packet([10, 88, 0, 2], [10, 88, 0, 3], b"revoked-a"),
            &mut tx,
        )
        .await;
        assert_no_inner(&sockets[1], address, &mut clients[1], &mut rx, &mut tx).await;
        send_inner(
            &sockets[2],
            address,
            &mut clients[2],
            &packet([10, 88, 0, 4], [10, 88, 0, 3], b"allowed-c"),
            &mut tx,
        )
        .await;
        assert_eq!(
            &timeout(
                Duration::from_secs(2),
                recv_inner(&sockets[1], &mut clients[1], &mut rx, &mut tx)
            )
            .await
            .unwrap()[28..],
            b"allowed-c"
        );
        task.abort();
    })
    .await
    .expect("bounded malformed snapshot recovery test");
}

async fn establish_client(
    socket: &UdpSocket,
    address: SocketAddr,
    client: &mut Tunn,
    tx: &mut [u8],
    rx: &mut [u8],
) {
    let TunnResult::WriteToNetwork(init) = client.encapsulate(&[], tx) else {
        panic!("expected handshake initiation")
    };
    socket.send_to(init, address).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(rx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(rx[..4].try_into().unwrap()),
        2,
        "hub responds to authenticated initiation"
    );
    if let TunnResult::WriteToNetwork(reply) = client.decapsulate(None, &rx[..n], tx) {
        socket.send_to(reply, address).await.unwrap();
    }
}

fn transport_checksum(src: [u8; 4], dst: [u8; 4], proto: u8, segment: &[u8]) -> u16 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&src);
    bytes.extend_from_slice(&dst);
    bytes.extend_from_slice(&[0, proto]);
    bytes.extend_from_slice(&(segment.len() as u16).to_be_bytes());
    bytes.extend_from_slice(segment);
    let mut sum = 0u32;
    for c in bytes.chunks_exact(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if bytes.len() % 2 != 0 {
        sum += (bytes[bytes.len() - 1] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn service_packet(
    proto: u8,
    src: [u8; 4],
    dst: [u8; 4],
    sport: u16,
    dport: u16,
    flags: u8,
    payload: &[u8],
) -> Vec<u8> {
    let transport_len = (if proto == 6 { 20 } else { 8 }) + payload.len();
    let mut p = vec![0; 20 + transport_len];
    let packet_len = p.len() as u16;
    p[0] = 0x45;
    p[2..4].copy_from_slice(&packet_len.to_be_bytes());
    p[8] = 64;
    p[9] = proto;
    p[12..16].copy_from_slice(&src);
    p[16..20].copy_from_slice(&dst);
    p[20..22].copy_from_slice(&sport.to_be_bytes());
    p[22..24].copy_from_slice(&dport.to_be_bytes());
    if proto == 6 {
        p[32] = 0x50;
        p[33] = flags;
        p[40..].copy_from_slice(payload);
    } else {
        p[24..26].copy_from_slice(&(transport_len as u16).to_be_bytes());
        p[28..].copy_from_slice(payload);
    }
    let c = transport_checksum(src, dst, proto, &p[20..]);
    if proto == 6 {
        p[36..38].copy_from_slice(&c.to_be_bytes());
    } else {
        p[26..28].copy_from_slice(&c.to_be_bytes());
    }
    let mut sum = 0u32;
    for c in p[..20].chunks_exact(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    p[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
    p
}

fn assert_packet_checksums(p: &[u8]) {
    let mut sum = 0u32;
    for c in p[..20].chunks_exact(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    assert_eq!(!(sum as u16), 0, "IPv4 checksum");
    let proto = p[9];
    let checksum = if proto == 6 {
        u16::from_be_bytes([p[36], p[37]])
    } else {
        u16::from_be_bytes([p[26], p[27]])
    };
    if proto == 6 || checksum != 0 {
        assert_eq!(
            transport_checksum(
                p[12..16].try_into().unwrap(),
                p[16..20].try_into().unwrap(),
                proto,
                &p[20..]
            ),
            0,
            "TCP/UDP checksum"
        );
    }
}

#[test]
fn shared_rate_limiter_cookie_challenge_accepts_same_source_retry() {
    let hub_secret = StaticSecret::from([201u8; 32]);
    let hub_public = PublicKey::from(&hub_secret);
    let limiter = Arc::new(RateLimiter::new(&hub_public, 0));
    let mut hub = Tunn::new(
        hub_secret,
        PublicKey::from(&StaticSecret::from([202u8; 32])),
        None,
        None,
        1,
        Some(limiter.clone()),
    );
    let mut client = Tunn::new(
        StaticSecret::from([202u8; 32]),
        hub_public,
        None,
        None,
        2,
        None,
    );
    let mut tx = vec![0; 65535];
    let mut out = vec![0; 65535];
    let TunnResult::WriteToNetwork(init) = client.encapsulate(&[], &mut tx) else {
        panic!("expected initiation")
    };
    let addr = "127.0.0.1:12345".parse::<SocketAddr>().unwrap();
    let TunnResult::WriteToNetwork(cookie) = limiter
        .verify_packet(Some(addr.ip()), init, &mut out)
        .unwrap_err()
    else {
        panic!("under-load initiation receives cookie challenge")
    };
    assert!(matches!(
        client.decapsulate(Some(addr.ip()), cookie, &mut tx),
        TunnResult::Done
    ));
    let TunnResult::WriteToNetwork(retry) = client.format_handshake_initiation(&mut tx, true)
    else {
        panic!("cookie retry initiation")
    };
    let packet = limiter
        .verify_packet(Some(addr.ip()), retry, &mut out)
        .expect("same limiter accepts valid MAC2 retry");
    assert!(matches!(packet, Packet::HandshakeInit(_)));
    assert!(matches!(
        hub.decapsulate(Some(addr.ip()), retry, &mut out),
        TunnResult::WriteToNetwork(_)
    ));
}

#[tokio::test]
async fn zero_peer_router_resets_shared_cookie_limiter_on_timer_and_reload() {
    let store = Arc::new(Store::open(":memory:").unwrap());
    store.bind_test_identity();
    let hub_private = [211u8; 32];
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store, hub_private)
        .await
        .unwrap();
    tokio::spawn(kernel.run());
    assert!(
        handle.is_ready(),
        "successful UDP startup with no setup is ready"
    );
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];

    // Saturate the router-owned limiter with valid MAC1 initiations while
    // there are no configured peers; the threshold challenge proves these
    // packets reached the shared cookie domain before any peer lookup.
    let mut challenged_client = Tunn::new(
        StaticSecret::from([212u8; 32]),
        hub_public,
        None,
        None,
        1,
        None,
    );
    for i in 0..110u8 {
        let mut client = Tunn::new(
            StaticSecret::from([i.wrapping_add(1); 32]),
            hub_public,
            None,
            None,
            10 + i as u32,
            None,
        );
        if let TunnResult::WriteToNetwork(init) = client.encapsulate(&[], &mut tx) {
            sender.send_to(init, address).await.unwrap();
        }
    }
    while timeout(Duration::from_millis(30), sender.recv_from(&mut rx))
        .await
        .is_ok()
    {}
    let TunnResult::WriteToNetwork(first) = challenged_client.encapsulate(&[], &mut tx) else {
        panic!("expected initiation")
    };
    sender.send_to(first, address).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(1), sender.recv_from(&mut rx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(rx[..4].try_into().unwrap()),
        3,
        "shared limiter emits a cookie under load"
    );
    assert!(matches!(
        challenged_client.decapsulate(Some(address.ip()), &rx[..n], &mut tx),
        TunnResult::Done
    ));
    // Drain load challenges before waiting for the one-second reset tick.
    while timeout(Duration::from_millis(30), sender.recv_from(&mut rx))
        .await
        .is_ok()
    {}
    time::sleep(Duration::from_millis(1100)).await;

    // The same router-owned limiter must have reset despite having zero
    // peers. An unknown peer is silently ignored after verification; a
    // stale limiter would instead answer with another cookie challenge.
    let mut after_tick = Tunn::new(
        StaticSecret::from([213u8; 32]),
        hub_public,
        None,
        None,
        222,
        None,
    );
    let TunnResult::WriteToNetwork(init) = after_tick.encapsulate(&[], &mut tx) else {
        panic!("expected post-tick initiation")
    };
    sender.send_to(init, address).await.unwrap();
    assert!(
        timeout(Duration::from_millis(150), sender.recv_from(&mut rx))
            .await
            .is_err(),
        "zero-peer timer tick resets the shared limiter"
    );

    // Reload does not replace the cookie domain: a retry carrying the
    // router-issued cookie remains verified after acknowledged reload.
    handle.reload().await.unwrap();
    // Re-saturate after the timer reset so this retry must authenticate its
    // MAC2 against the original cookie domain rather than pass under load.
    for i in 0..110u8 {
        let mut client = Tunn::new(
            StaticSecret::from([i.wrapping_add(31); 32]),
            hub_public,
            None,
            None,
            300 + i as u32,
            None,
        );
        if let TunnResult::WriteToNetwork(init) = client.encapsulate(&[], &mut tx) {
            sender.send_to(init, address).await.unwrap();
        }
    }
    while timeout(Duration::from_millis(30), sender.recv_from(&mut rx))
        .await
        .is_ok()
    {}
    let TunnResult::WriteToNetwork(retry) =
        challenged_client.format_handshake_initiation(&mut tx, true)
    else {
        panic!("cookie retry initiation")
    };
    sender.send_to(retry, address).await.unwrap();
    assert!(
        timeout(Duration::from_millis(150), sender.recv_from(&mut rx))
            .await
            .is_err(),
        "reload retained the cookie limiter and accepted its same-source retry"
    );
}

async fn send_inner(
    socket: &UdpSocket,
    address: SocketAddr,
    client: &mut Tunn,
    packet: &[u8],
    tx: &mut [u8],
) {
    let TunnResult::WriteToNetwork(wire) = client.encapsulate(packet, tx) else {
        panic!("expected encrypted packet")
    };
    socket.send_to(wire, address).await.unwrap();
}

async fn recv_inner(
    socket: &UdpSocket,
    client: &mut Tunn,
    rx: &mut [u8],
    tx: &mut [u8],
) -> Vec<u8> {
    loop {
        let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(rx))
            .await
            .unwrap()
            .unwrap();
        match client.decapsulate(None, &rx[..n], tx) {
            TunnResult::WriteToTunnelV4(p, _) => return p.to_vec(),
            TunnResult::WriteToNetwork(p) => {
                /* handshake response; caller needs a socket to send it */
                let _ = p;
                panic!("unexpected hub handshake while awaiting packet");
            }
            _ => {}
        }
    }
}

async fn assert_no_inner(
    socket: &UdpSocket,
    address: SocketAddr,
    client: &mut Tunn,
    rx: &mut [u8],
    tx: &mut [u8],
) {
    let until = tokio::time::Instant::now() + Duration::from_millis(250);
    loop {
        let remaining = until.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return;
        }
        let Ok(Ok((n, _))) = timeout(remaining, socket.recv_from(rx)).await else {
            return;
        };
        match client.decapsulate(None, &rx[..n], tx) {
            TunnResult::WriteToTunnelV4(_, _) => panic!("denied flow delivered application IPv4"),
            TunnResult::WriteToNetwork(packet) => {
                let _ = socket.send_to(packet, address).await;
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn boringtun_nat_tcp_udp_roundtrip_and_live_policy_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("nat-e2e.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("192.168.44.0/24", "hub.example:51820", 25)
        .unwrap();
    for g in [
        Group {
            id: "clients".into(),
            name: "clients".into(),
            allowed_groups: vec!["backend".into()],
        },
        Group {
            id: "backend".into(),
            name: "backend".into(),
            allowed_groups: vec![],
        },
        Group {
            id: "denied".into(),
            name: "denied".into(),
            allowed_groups: vec![],
        },
    ] {
        store.add_group(&g).unwrap();
    }
    store.set_acl("clients", &["backend".into()]).unwrap();
    // Isolate the forward allowlist assertion: C's group ACL itself permits
    // access to the backend, but its group is not in the forward allowlist.
    store.set_acl("denied", &["backend".into()]).unwrap();
    let secrets = [
        StaticSecret::from([31u8; 32]),
        StaticSecret::from([32u8; 32]),
        StaticSecret::from([33u8; 32]),
        StaticSecret::from([34u8; 32]),
    ];
    let specs = [
        ("a", "192.168.44.2", "clients"),
        ("b", "192.168.44.3", "backend"),
        ("c", "192.168.44.4", "denied"),
        ("d", "192.168.44.5", "backend"),
    ];
    for ((id, ip, group), secret) in specs.iter().zip(secrets.iter()) {
        store
            .add_peer(&Peer {
                id: (*id).into(),
                name: (*id).into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: (*ip).into(),
                group_id: (*group).into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    create_forward_for_test(
        &store,
        Forward {
            id: "f".into(),
            name: "service".into(),
            protocol: "tcp".into(),
            target_peer_id: "b".into(),
            target_port: 8080,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    create_forward_for_test(
        &store,
        Forward {
            id: "fu".into(),
            name: "udp service".into(),
            protocol: "udp".into(),
            target_peer_id: "b".into(),
            target_port: 8080,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    create_forward_for_test(
        &store,
        Forward {
            id: "reserved".into(),
            name: "reserved tcp service".into(),
            protocol: "tcp".into(),
            target_peer_id: "b".into(),
            target_port: 40000,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    let hub_private = [35u8; 32];
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(kernel.run());
    let sockets = [
        UdpSocket::bind("127.0.0.1:0").await.unwrap(),
        UdpSocket::bind("127.0.0.1:0").await.unwrap(),
        UdpSocket::bind("127.0.0.1:0").await.unwrap(),
        UdpSocket::bind("127.0.0.1:0").await.unwrap(),
    ];
    let mut clients: Vec<Tunn> = secrets
        .into_iter()
        .enumerate()
        .map(|(i, s)| Tunn::new(s, hub_public, None, None, 60 + i as u32, None))
        .collect();
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];
    for i in 0..4 {
        establish_client(&sockets[i], address, &mut clients[i], &mut tx, &mut rx).await;
    }

    let mut translated_reply = None;
    let mut total_request_bytes = 0u64;
    let mut total_reply_bytes = 0u64;
    let mut requests = Vec::new();
    for (_proto, number, flags) in [("tcp", 6, 0x02), ("udp", 17, 0)] {
        let req = service_packet(
            number,
            [192, 168, 44, 2],
            [192, 168, 44, 1],
            12345,
            8080,
            flags,
            b"request",
        );
        total_request_bytes += req.len() as u64;
        let src_peer = store
            .peers()
            .unwrap()
            .into_iter()
            .find(|p| p.id == "a")
            .unwrap();
        let source_group = store.group("clients").unwrap().unwrap();
        assert!(
            ipv4::validate_and_forward(&req, &src_peer, Some(&source_group)).is_some(),
            "fixture must pass authenticated IPv4 validation"
        );
        send_inner(&sockets[0], address, &mut clients[0], &req, &mut tx).await;
        requests.push((number, req));
    }
    // Both requests are outstanding before either backend reply arrives.
    let mut mapped_requests = Vec::new();
    for (number, req) in &requests {
        let translated = recv_inner(&sockets[1], &mut clients[1], &mut rx, &mut tx).await;
        assert_eq!(&translated[12..16], &[192, 168, 44, 1]);
        assert_eq!(&translated[16..20], &[192, 168, 44, 3]);
        let snat_port = u16::from_be_bytes([translated[20], translated[21]]);
        assert_eq!(
            snat_port,
            if *number == 6 { 40001 } else { 40000 },
            "initial protocol-specific SNAT allocation"
        );
        assert_eq!(u16::from_be_bytes([translated[22], translated[23]]), 8080);
        assert_eq!(
            translated[8], 63,
            "SNAT/DNAT must not decrement TTL a second time"
        );
        assert_packet_checksums(&translated);
        assert_eq!(translated[9], *number);
        let payload_at = if *number == 6 { 40 } else { 28 };
        assert_eq!(&translated[payload_at..], &req[payload_at..]);
        mapped_requests.push((*number, snat_port));
    }
    for (number, snat_port) in mapped_requests.iter().rev() {
        let reply = service_packet(
            *number,
            [192, 168, 44, 3],
            [192, 168, 44, 1],
            8080,
            *snat_port,
            if *number == 6 { 0x12 } else { 0 },
            b"reply",
        );
        total_reply_bytes += reply.len() as u64;
        translated_reply = Some(reply.clone());
        send_inner(&sockets[1], address, &mut clients[1], &reply, &mut tx).await;
    }
    for (number, _) in requests.iter().rev() {
        let restored = recv_inner(&sockets[0], &mut clients[0], &mut rx, &mut tx).await;
        assert_eq!(&restored[12..16], &[192, 168, 44, 1]);
        assert_eq!(&restored[16..20], &[192, 168, 44, 2]);
        assert_eq!(u16::from_be_bytes([restored[20], restored[21]]), 8080);
        assert_eq!(u16::from_be_bytes([restored[22], restored[23]]), 12345);
        assert_eq!(restored[9], *number);
        let payload_at = if *number == 6 { 40 } else { 28 };
        assert_eq!(&restored[payload_at..], b"reply");
        assert_eq!(restored[8], 63);
        assert_packet_checksums(&restored);
    }
    // Warm direct UDP is stateful in both directions and admits only the
    // exact authenticated reverse tuple.
    let direct = service_packet(
        17,
        [192, 168, 44, 2],
        [192, 168, 44, 3],
        12345,
        9090,
        0,
        b"direct",
    );
    send_inner(&sockets[0], address, &mut clients[0], &direct, &mut tx).await;
    let delivered = recv_inner(&sockets[1], &mut clients[1], &mut rx, &mut tx).await;
    assert_eq!(&delivered[12..20], &direct[12..20]);
    total_request_bytes += direct.len() as u64;
    let direct_reply = service_packet(
        17,
        [192, 168, 44, 3],
        [192, 168, 44, 2],
        9090,
        12345,
        0,
        b"direct-reply",
    );
    send_inner(
        &sockets[1],
        address,
        &mut clients[1],
        &direct_reply,
        &mut tx,
    )
    .await;
    let restored = recv_inner(&sockets[0], &mut clients[0], &mut rx, &mut tx).await;
    assert_eq!(&restored[12..20], &direct_reply[12..20]);
    total_reply_bytes += direct_reply.len() as u64;
    for invalid in [
        service_packet(
            17,
            [192, 168, 44, 3],
            [192, 168, 44, 2],
            9091,
            12345,
            0,
            b"changed-source-port",
        ),
        service_packet(
            17,
            [192, 168, 44, 3],
            [192, 168, 44, 2],
            9090,
            12346,
            0,
            b"changed-destination-port",
        ),
    ] {
        send_inner(&sockets[1], address, &mut clients[1], &invalid, &mut tx).await;
        assert_no_inner(&sockets[0], address, &mut clients[0], &mut rx, &mut tx).await;
    }
    // Counters are plaintext IPv4 bytes delivered across the hub boundary:
    // ingress accounts on the source peer, egress on the destination peer.
    let snapshot = handle.stats().await;
    let a_rx = snapshot["a"].rx_bytes;
    let a_tx = snapshot["a"].tx_bytes;
    let b_rx = snapshot["b"].rx_bytes;
    let b_tx = snapshot["b"].tx_bytes;
    assert_eq!(
        a_rx, total_request_bytes,
        "A ingress includes both delivered forward requests"
    );
    assert_eq!(
        a_tx, total_reply_bytes,
        "A egress includes both delivered reverse replies"
    );
    assert_eq!(
        b_rx, total_reply_bytes,
        "B ingress includes both delivered reverse replies"
    );
    assert_eq!(
        b_tx, total_request_bytes,
        "B egress includes both delivered forward requests"
    );
    drop(snapshot);

    // A backend error is related traffic even without a reverse group ACL.
    let udp_quote = service_packet(
        17,
        [192, 168, 44, 1],
        [192, 168, 44, 3],
        40000,
        8080,
        0,
        b"request",
    );
    let icmp_error = ipv4::test_icmp_error(
        Ipv4Addr::new(192, 168, 44, 3),
        Ipv4Addr::new(192, 168, 44, 1),
        3,
        4,
        &udp_quote[..28],
    );
    send_inner(&sockets[1], address, &mut clients[1], &icmp_error, &mut tx).await;
    let restored = recv_inner(&sockets[0], &mut clients[0], &mut rx, &mut tx).await;
    assert_eq!(&restored[12..20], &[192, 168, 44, 1, 192, 168, 44, 2]);
    assert_eq!(&restored[40..48], &[192, 168, 44, 2, 192, 168, 44, 1]);
    assert_eq!(u16::from_be_bytes([restored[48], restored[49]]), 12345);
    assert_eq!(u16::from_be_bytes([restored[26], restored[27]]), 1280);
    assert_eq!(checksum(&restored[..20]), 0);
    assert_eq!(checksum(&restored[20..]), 0);
    assert_eq!(checksum(&restored[28..48]), 0);
    let wrong_peer_error = ipv4::test_icmp_error(
        Ipv4Addr::new(192, 168, 44, 5),
        Ipv4Addr::new(192, 168, 44, 1),
        3,
        4,
        &udp_quote[..28],
    );
    send_inner(
        &sockets[3],
        address,
        &mut clients[3],
        &wrong_peer_error,
        &mut tx,
    )
    .await;
    assert_no_inner(&sockets[0], address, &mut clients[0], &mut rx, &mut tx).await;

    // A newly added TCP service is installed only after acknowledged reload;
    // existing authorized mappings remain live and the next TCP candidate is excluded.
    create_forward_for_test(
        &store,
        Forward {
            id: "reserved-next".into(),
            name: "next reserved tcp service".into(),
            protocol: "tcp".into(),
            target_peer_id: "b".into(),
            target_port: 40001,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    handle.reload().await.unwrap();
    clients[0] = Tunn::new(
        StaticSecret::from([31u8; 32]),
        hub_public,
        None,
        None,
        60,
        None,
    );
    clients[1] = Tunn::new(
        StaticSecret::from([32u8; 32]),
        hub_public,
        None,
        None,
        61,
        None,
    );
    establish_client(&sockets[0], address, &mut clients[0], &mut tx, &mut rx).await;
    establish_client(&sockets[1], address, &mut clients[1], &mut tx, &mut rx).await;
    clients[2] = Tunn::new(
        StaticSecret::from([33u8; 32]),
        hub_public,
        None,
        None,
        62,
        None,
    );
    clients[3] = Tunn::new(
        StaticSecret::from([34u8; 32]),
        hub_public,
        None,
        None,
        63,
        None,
    );
    establish_client(&sockets[2], address, &mut clients[2], &mut tx, &mut rx).await;
    establish_client(&sockets[3], address, &mut clients[3], &mut tx, &mut rx).await;
    // The newly configured TCP service claims the old TCP mapping's SNAT
    // port (40001), so that reply must no longer reach A. The UDP mapping
    // uses a protocol-independent port allocation (40000) and remains valid.
    let stale_tcp_reply = service_packet(
        6,
        [192, 168, 44, 3],
        [192, 168, 44, 1],
        8080,
        40001,
        0x12,
        b"reply",
    );
    send_inner(
        &sockets[1],
        address,
        &mut clients[1],
        &stale_tcp_reply,
        &mut tx,
    )
    .await;
    assert_no_inner(&sockets[0], address, &mut clients[0], &mut rx, &mut tx).await;
    let retained_udp_reply = service_packet(
        17,
        [192, 168, 44, 3],
        [192, 168, 44, 1],
        8080,
        40000,
        0,
        b"reply",
    );
    send_inner(
        &sockets[1],
        address,
        &mut clients[1],
        &retained_udp_reply,
        &mut tx,
    )
    .await;
    let retained_reply = recv_inner(&sockets[0], &mut clients[0], &mut rx, &mut tx).await;
    assert_eq!(
        &retained_reply[28..],
        b"reply",
        "non-colliding UDP mapping remains live"
    );
    for (number, flags) in [(6, 0x02), (17, 0)] {
        let request = service_packet(
            number,
            [192, 168, 44, 2],
            [192, 168, 44, 1],
            12346,
            8080,
            flags,
            b"after-reload",
        );
        send_inner(&sockets[0], address, &mut clients[0], &request, &mut tx).await;
        let translated = recv_inner(&sockets[1], &mut clients[1], &mut rx, &mut tx).await;
        let snat = u16::from_be_bytes([translated[20], translated[21]]);
        if number == 6 {
            assert!(
                ![40000, 40001].contains(&snat),
                "reserved TCP service ports are excluded"
            );
        } else {
            assert_eq!(
                snat, 40001,
                "UDP SNAT cursor is retained across reload and remains protocol-specific"
            );
        }
    }

    // C has a directed backend ACL, but the current forward allowlist excludes it.
    let denied = service_packet(
        17,
        [192, 168, 44, 4],
        [192, 168, 44, 1],
        2345,
        8080,
        0,
        b"denied",
    );
    send_inner(&sockets[2], address, &mut clients[2], &denied, &mut tx).await;
    assert!(
        timeout(Duration::from_millis(200), sockets[1].recv_from(&mut rx))
            .await
            .is_err()
    );
    // D is freshly authenticated after policy reload, but spoofing B's inner
    // source is rejected before reverse NAT.
    let spoof = service_packet(
        17,
        [192, 168, 44, 3],
        [192, 168, 44, 1],
        8080,
        40000,
        0,
        b"spoof",
    );
    send_inner(&sockets[3], address, &mut clients[3], &spoof, &mut tx).await;
    assert!(
        timeout(Duration::from_millis(200), sockets[0].recv_from(&mut rx))
            .await
            .is_err()
    );

    // ACL revoke is synchronously acknowledged and flushes NAT state.
    store.set_acl("clients", &[]).unwrap();
    handle.reload().await.unwrap();
    clients[0] = Tunn::new(
        StaticSecret::from([31u8; 32]),
        hub_public,
        None,
        None,
        60,
        None,
    );
    clients[1] = Tunn::new(
        StaticSecret::from([32u8; 32]),
        hub_public,
        None,
        None,
        61,
        None,
    );
    establish_client(&sockets[0], address, &mut clients[0], &mut tx, &mut rx).await;
    establish_client(&sockets[1], address, &mut clients[1], &mut tx, &mut rx).await;
    // The reload must clear established NAT mappings: a reply translated for
    // the previous flow cannot be delivered to A using its stale mapping.
    send_inner(
        &sockets[1],
        address,
        &mut clients[1],
        translated_reply.as_ref().unwrap(),
        &mut tx,
    )
    .await;
    assert_no_inner(&sockets[0], address, &mut clients[0], &mut rx, &mut tx).await;
    send_inner(&sockets[1], address, &mut clients[1], &icmp_error, &mut tx).await;
    assert_no_inner(&sockets[0], address, &mut clients[0], &mut rx, &mut tx).await;
    send_inner(
        &sockets[0],
        address,
        &mut clients[0],
        &service_packet(
            17,
            [192, 168, 44, 2],
            [192, 168, 44, 1],
            12345,
            8080,
            0,
            b"revoked",
        ),
        &mut tx,
    )
    .await;
    assert_no_inner(&sockets[1], address, &mut clients[1], &mut rx, &mut tx).await;
    // Restore ACL, then remove the forward and prove its acknowledged removal blocks traffic.
    store.set_acl("clients", &["backend".into()]).unwrap();
    store.remove_forward("fu").unwrap();
    handle.reload().await.unwrap();
    clients[0] = Tunn::new(
        StaticSecret::from([31u8; 32]),
        hub_public,
        None,
        None,
        60,
        None,
    );
    establish_client(&sockets[0], address, &mut clients[0], &mut tx, &mut rx).await;
    send_inner(
        &sockets[0],
        address,
        &mut clients[0],
        &service_packet(
            17,
            [192, 168, 44, 2],
            [192, 168, 44, 1],
            12345,
            8080,
            0,
            b"removed",
        ),
        &mut tx,
    )
    .await;
    assert_no_inner(&sockets[1], address, &mut clients[1], &mut rx, &mut tx).await;
}

#[tokio::test]
async fn only_authenticated_keepalive_can_migrate_peer_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(Store::open(dir.path().join("endpoint.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store.setup("10.1.0.0/24", "hub.example:51820", 25).unwrap();
    store
        .add_group(&Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "b".into(),
            name: "B".into(),
            allowed_groups: vec!["a".into()],
        })
        .unwrap();
    store.set_acl("b", &["a".into()]).unwrap();
    let a_secret = StaticSecret::from([17u8; 32]);
    let b_secret = StaticSecret::from([18u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &a_secret, "10.1.0.2", "a"),
        ("b", &b_secret, "10.1.0.3", "b"),
    ] {
        store
            .add_peer(&Peer {
                id: id.into(),
                name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(),
                group_id: group.into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    let hub_private = [19u8; 32];
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store, hub_private)
        .await
        .unwrap();
    let task = tokio::spawn(kernel.run());
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let original = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let migrated = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let attacker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret, hub_public, None, None, 51, None);
    let mut b = Tunn::new(b_secret, hub_public, None, None, 52, None);
    let mut tx = vec![0u8; 65535];
    let mut rx = vec![0u8; 65535];
    establish_client(&original, address, &mut a, &mut tx, &mut rx).await;
    establish_client(&b_socket, address, &mut b, &mut tx, &mut rx).await;

    let TunnResult::WriteToNetwork(keepalive) = a.encapsulate(&[], &mut tx) else {
        panic!("expected encrypted keepalive")
    };
    let keepalive = keepalive.to_vec();
    migrated.send_to(&keepalive, address).await.unwrap();
    time::sleep(Duration::from_millis(50)).await;
    let route_to_a = ipv4::test_packet([10, 1, 0, 3], [10, 1, 0, 2], false, false);
    let TunnResult::WriteToNetwork(wire) = b.encapsulate(&route_to_a, &mut tx) else {
        panic!("expected encrypted routed packet")
    };
    b_socket.send_to(wire, address).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(3), migrated.recv_from(&mut rx))
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            a.decapsulate(None, &rx[..n], &mut tx),
            TunnResult::WriteToTunnelV4(_, _)
        ),
        "hub uses the new endpoint after authenticated keepalive"
    );
    assert!(
        timeout(Duration::from_millis(100), original.recv_from(&mut rx))
            .await
            .is_err(),
        "hub stopped using the old endpoint"
    );

    let TunnResult::WriteToNetwork(forged) = a.encapsulate(&[], &mut tx) else {
        panic!("expected keepalive for forgery")
    };
    let mut forged = forged.to_vec();
    *forged.last_mut().unwrap() ^= 1;
    attacker.send_to(&forged, address).await.unwrap();
    time::sleep(Duration::from_millis(50)).await;
    attacker.send_to(&keepalive, address).await.unwrap();
    time::sleep(Duration::from_millis(50)).await;
    let TunnResult::WriteToNetwork(wire) = b.encapsulate(&route_to_a, &mut tx) else {
        panic!("expected encrypted routed packet")
    };
    b_socket.send_to(wire, address).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(3), migrated.recv_from(&mut rx))
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            a.decapsulate(None, &rx[..n], &mut tx),
            TunnResult::WriteToTunnelV4(_, _)
        ),
        "invalid and replayed packets cannot hijack the endpoint"
    );
    assert!(
        timeout(Duration::from_millis(100), attacker.recv_from(&mut rx))
            .await
            .is_err(),
        "forged/replayed sender receives no routed packet"
    );
    drop(handle);
    assert!(matches!(task.await.unwrap(), Err(RunError::Stopped)));
}

#[tokio::test]
async fn router_publishes_stats_only_for_authenticated_udp_packets() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(Store::open(dir.path().join("endpoint.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store.setup("10.1.0.0/24", "hub.example:51820", 25).unwrap();
    store
        .add_group(&Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let a_secret = StaticSecret::from([17u8; 32]);
    store
        .add_peer(&Peer {
            id: "a".into(),
            name: "a".into(),
            public_key: base64::engine::general_purpose::STANDARD
                .encode(PublicKey::from(&a_secret).as_bytes()),
            ipv4: "10.1.0.2".into(),
            group_id: "a".into(),
            received_bytes: 0,
            sent_bytes: 0,
            last_handshake_unix: None,
        })
        .unwrap();
    let hub_private = [19u8; 32];
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (publish_tx, mut publish_rx) = mpsc::unbounded_channel();
    let (packet_tx, mut packet_rx) = mpsc::unbounded_channel();
    let (kernel, handle) = initialize_store_runtime(server, store, hub_private)
        .await
        .unwrap();
    let runtime = kernel.run();
    tokio::spawn(
        STATS_PUBLISH_COUNT.scope(
            std::cell::Cell::new(0),
            DISABLE_TIMERS_FOR_TEST.scope(
                true,
                PACKET_RESULT_OBSERVER
                    .scope(packet_tx, STATS_PUBLISH_OBSERVER.scope(publish_tx, runtime)),
            ),
        ),
    );
    // Initial statistics are published by initialize, before runtime task-local observers exist.
    let startup_publications = 0;
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let original = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let migrated = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let attacker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret, hub_public, None, None, 51, None);
    let mut tx = vec![0u8; 65535];
    let mut rx = vec![0u8; 65535];

    // One installed peer receives index 0x200 (allocate_index starts at 2). This is a
    // syntactically valid Data packet whose receiver index hits, but has no session yet.
    let mut no_session = vec![0u8; 32];
    no_session[..4].copy_from_slice(&4u32.to_le_bytes());
    no_session[4..8].copy_from_slice(&0x200u32.to_le_bytes());
    attacker.send_to(&no_session, address).await.unwrap();
    assert!(
        !timeout(Duration::from_secs(1), packet_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        "index-hit NoCurrentSession Data must be rejected"
    );
    assert!(
        publish_rx.try_recv().is_err(),
        "NoCurrentSession Data must not publish stats"
    );

    establish_client(&original, address, &mut a, &mut tx, &mut rx).await;
    assert!(
        timeout(Duration::from_secs(1), packet_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        "authenticated initiation must be accepted"
    );
    assert_eq!(
        timeout(Duration::from_secs(1), publish_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        startup_publications + 1
    );
    assert!(
        timeout(Duration::from_secs(1), packet_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        "authenticated client keepalive completing the handshake must be accepted"
    );
    assert_eq!(
        timeout(Duration::from_secs(1), publish_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        startup_publications + 2
    );
    assert!(
        handle.stats().await["a"].last_handshake_unix.is_some(),
        "valid handshake updates published handshake stats"
    );

    // Valid keepalive from a different endpoint is authenticated, published and migrates endpoint.
    let TunnResult::WriteToNetwork(keepalive) = a.encapsulate(&[], &mut tx) else {
        panic!("expected encrypted keepalive")
    };
    let keepalive = keepalive.to_vec();
    migrated.send_to(&keepalive, address).await.unwrap();
    assert!(timeout(Duration::from_secs(1), packet_rx.recv())
        .await
        .unwrap()
        .unwrap());
    assert_eq!(
        timeout(Duration::from_secs(1), publish_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        startup_publications + 3
    );
    assert!(
        handle.stats().await["a"].last_data_unix.is_some(),
        "valid keepalive updates published data stats"
    );

    // Valid encrypted data also traverses the real router packet path and publishes.
    let inner = ipv4::test_packet([10, 1, 0, 2], [10, 1, 0, 99], false, false);
    let TunnResult::WriteToNetwork(data) = a.encapsulate(&inner, &mut tx) else {
        panic!("expected encrypted data")
    };
    original.send_to(data, address).await.unwrap();
    assert!(timeout(Duration::from_secs(1), packet_rx.recv())
        .await
        .unwrap()
        .unwrap());
    assert_eq!(
        timeout(Duration::from_secs(1), publish_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        startup_publications + 4
    );

    // Bad tag and exact replay both reach the indexed peer but must not publish.
    let TunnResult::WriteToNetwork(forged) = a.encapsulate(&[], &mut tx) else {
        panic!("expected keepalive for forgery")
    };
    let mut forged = forged.to_vec();
    *forged.last_mut().unwrap() ^= 1;
    attacker.send_to(&forged, address).await.unwrap();
    assert!(
        !timeout(Duration::from_secs(1), packet_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        "bad authentication tag must be rejected"
    );
    assert!(
        publish_rx.try_recv().is_err(),
        "bad tag must not publish stats"
    );

    let TunnResult::WriteToNetwork(replay) = a.encapsulate(&[], &mut tx) else {
        panic!("expected replay packet")
    };
    let replay = replay.to_vec();
    original.send_to(&replay, address).await.unwrap();
    assert!(timeout(Duration::from_secs(1), packet_rx.recv())
        .await
        .unwrap()
        .unwrap());
    assert_eq!(
        timeout(Duration::from_secs(1), publish_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        startup_publications + 5
    );
    original.send_to(&replay, address).await.unwrap();
    assert!(
        !timeout(Duration::from_secs(1), packet_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        "exact replay must be rejected"
    );
    assert!(
        publish_rx.try_recv().is_err(),
        "exact replay must not publish stats"
    );
    drop(handle);
}

#[tokio::test]
async fn unrelated_reload_preserves_established_client_tunnels_and_direct_udp_flow() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("retain.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("10.91.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec!["b".into()],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "b".into(),
            name: "B".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store.set_acl("a", &["b".into()]).unwrap();
    let a_secret = StaticSecret::from([171u8; 32]);
    let b_secret = StaticSecret::from([172u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &a_secret, "10.91.0.2", "a"),
        ("b", &b_secret, "10.91.0.3", "b"),
    ] {
        store
            .add_peer(&Peer {
                id: id.into(),
                name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(),
                group_id: group.into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    let hub_private = [173u8; 32];
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(kernel.run());
    let a_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret, hub_public, None, None, 71, None);
    let mut b = Tunn::new(b_secret, hub_public, None, None, 72, None);
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];
    establish_client(&a_socket, address, &mut a, &mut tx, &mut rx).await;
    establish_client(&b_socket, address, &mut b, &mut tx, &mut rx).await;
    let request = service_packet(
        17,
        [10, 91, 0, 2],
        [10, 91, 0, 3],
        12000,
        9000,
        0,
        b"before-reload",
    );
    send_inner(&a_socket, address, &mut a, &request, &mut tx).await;
    assert_eq!(
        &recv_inner(&b_socket, &mut b, &mut rx, &mut tx).await[28..],
        b"before-reload"
    );
    store
        .add_group(&Group {
            id: "unrelated".into(),
            name: "unrelated".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    handle.reload().await.unwrap();

    // Keep the exact same client Tunn objects and use the established UDP tuple.
    let continuation = service_packet(
        17,
        [10, 91, 0, 2],
        [10, 91, 0, 3],
        12000,
        9000,
        0,
        b"after-reload",
    );
    send_inner(&a_socket, address, &mut a, &continuation, &mut tx).await;
    let delivered = timeout(
        Duration::from_millis(500),
        recv_inner(&b_socket, &mut b, &mut rx, &mut tx),
    )
    .await;
    assert!(
        delivered.is_ok(),
        "unrelated reload retains receiver index, tunnel session, and established direct UDP flow"
    );
    assert_eq!(&delivered.unwrap()[28..], b"after-reload");
    let snapshot = handle.stats().await;
    assert!(
        snapshot["a"].rx_bytes >= (request.len() + continuation.len()) as u64,
        "application counters remain cumulative over reload"
    );
}

#[tokio::test]
async fn boringtun_clients_handshake_and_route_only_authorized_ipv4() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("test.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("10.77.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec!["b".into()],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "b".into(),
            name: "B".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store.set_acl("a", &["b".into()]).unwrap();
    let client_a_secret = StaticSecret::from([7u8; 32]);
    let client_b_secret = StaticSecret::from([8u8; 32]);
    let client_c_secret = StaticSecret::from([10u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &client_a_secret, "10.77.0.2", "a"),
        ("b", &client_b_secret, "10.77.0.3", "b"),
        ("c", &client_c_secret, "10.77.0.4", "a"),
    ] {
        let public = PublicKey::from(secret);
        let peer = Peer {
            id: id.into(),
            name: id.into(),
            public_key: base64::engine::general_purpose::STANDARD.encode(public.as_bytes()),
            ipv4: ip.into(),
            group_id: group.into(),
            received_bytes: 0,
            sent_bytes: 0,
            last_handshake_unix: None,
        };
        store.add_peer(&peer).unwrap();
    }
    let hub_private = [9u8; 32];
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(kernel.run());
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let client_a_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_b_migrated_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_c_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut client_a = Tunn::new(client_a_secret.clone(), hub_public, None, None, 33, None);
    let mut client_b = Tunn::new(client_b_secret.clone(), hub_public, None, None, 34, None);
    let mut client_c = Tunn::new(client_c_secret, hub_public, None, None, 35, None);
    let mut tx = vec![0u8; 65535];
    let mut rx = vec![0u8; 65535];
    for (socket, client) in [
        (&client_a_socket, &mut client_a),
        (&client_b_socket, &mut client_b),
    ] {
        let TunnResult::WriteToNetwork(init) = client.encapsulate(&[], &mut tx) else {
            panic!("expected handshake initiation")
        };
        socket.send_to(init, address).await.unwrap();
        let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(&mut rx))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(rx[..4].try_into().unwrap()),
            2,
            "server must return a handshake response"
        );
        let result = client.decapsulate(None, &rx[..n], &mut tx);
        match result {
            TunnResult::Done => {}
            TunnResult::WriteToNetwork(packet) => {
                socket.send_to(packet, address).await.unwrap();
            }
            other => panic!("client rejected handshake response: {other:?}"),
        }
    }
    let TunnResult::WriteToNetwork(init) = client_c.encapsulate(&[], &mut tx) else {
        panic!("expected client C handshake initiation")
    };
    client_c_socket.send_to(init, address).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(3), client_c_socket.recv_from(&mut rx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(rx[..4].try_into().unwrap()),
        2,
        "server must return client C handshake response"
    );
    if let TunnResult::WriteToNetwork(packet) = client_c.decapsulate(None, &rx[..n], &mut tx) {
        client_c_socket.send_to(packet, address).await.unwrap();
    }

    // An unrelated mutation retains the exact established tunnel sessions.
    store
        .add_group(&Group {
            id: "unrelated".into(),
            name: "unrelated".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    handle.reload().await.unwrap();
    // A valid transport packet from a migrated socket still authenticates
    // endpoint migration independently of the reload.
    let TunnResult::WriteToNetwork(keepalive) = client_b.encapsulate(&[], &mut tx) else {
        panic!("expected B keepalive")
    };
    client_b_migrated_socket
        .send_to(keepalive, address)
        .await
        .unwrap();

    let packet = ipv4::test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
    let TunnResult::WriteToNetwork(wire) = client_a.encapsulate(&packet, &mut tx) else {
        panic!("expected encrypted data")
    };
    client_a_socket.send_to(wire, address).await.unwrap();
    let first_inner = recv_inner(&client_b_migrated_socket, &mut client_b, &mut rx, &mut tx).await;
    assert_eq!(&first_inner[12..20], &packet[12..20]);
    assert_eq!(
        first_inner[8], 63,
        "pending forwarding decrements TTL only once"
    );
    let after_data = handle.stats().await;
    assert_eq!(after_data["a"].rx_bytes, packet.len() as u64);
    assert_eq!(after_data["b"].tx_bytes, packet.len() as u64);
    drop(after_data);

    // The pending delivery used the authenticated response endpoint; warm
    // traffic continues to route there without another handshake.
    let second_packet = ipv4::test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false);
    let TunnResult::WriteToNetwork(wire) = client_a.encapsulate(&second_packet, &mut tx) else {
        panic!("expected second encrypted packet")
    };
    client_a_socket.send_to(wire, address).await.unwrap();
    let second_inner = loop {
        let (n, _) = timeout(
            Duration::from_secs(3),
            client_b_migrated_socket.recv_from(&mut rx),
        )
        .await
        .unwrap()
        .unwrap();
        match client_b.decapsulate(None, &rx[..n], &mut tx) {
            TunnResult::WriteToTunnelV4(packet, _) => break packet.to_vec(),
            TunnResult::WriteToNetwork(reply) => {
                client_b_migrated_socket
                    .send_to(reply, address)
                    .await
                    .unwrap();
            }
            _ => {}
        }
    };
    assert_eq!(&second_inner[12..20], &second_packet[12..20]);
    assert_eq!(second_inner[8], 63, "forwarding decrements TTL");
    assert!(
        timeout(
            Duration::from_millis(100),
            client_b_socket.recv_from(&mut rx)
        )
        .await
        .is_err(),
        "hub continued using the old destination endpoint"
    );

    // Reverse direction is denied, as are same-group routing, source spoofing,
    // malformed headers and both first and non-first IPv4 fragments.
    for (source, destination, _same_group, _spoof, malformed, fragment) in [
        ([10, 77, 0, 3], [10, 77, 0, 2], false, false, false, false),
        ([10, 77, 0, 2], [10, 77, 0, 4], true, false, false, false),
        ([10, 77, 0, 99], [10, 77, 0, 3], false, true, false, false),
        ([10, 77, 0, 2], [10, 77, 0, 3], false, false, true, false),
        ([10, 77, 0, 2], [10, 77, 0, 3], false, false, false, true),
    ] {
        let (socket, client) = if source[3] == 3 {
            (&client_b_migrated_socket, &mut client_b)
        } else {
            (&client_a_socket, &mut client_a)
        };
        let packet = ipv4::test_packet(source, destination, malformed, fragment);
        let TunnResult::WriteToNetwork(wire) = client.encapsulate(&packet, &mut tx) else {
            panic!("expected encrypted packet")
        };
        socket.send_to(wire, address).await.unwrap();
        let (denied_socket, denied_client) = match destination[3] {
            2 => (&client_a_socket, &mut client_a),
            3 => (&client_b_migrated_socket, &mut client_b),
            4 => (&client_c_socket, &mut client_c),
            _ => unreachable!(),
        };
        assert_no_inner(denied_socket, address, denied_client, &mut rx, &mut tx).await;
    }

    // A successful acknowledgement means the old tunnels and policy are gone.
    store.set_acl("a", &[]).unwrap();
    handle.reload().await.unwrap();
    // The initiating client establishes a fresh session after policy reload.
    client_a = Tunn::new(client_a_secret, hub_public, None, None, 33, None);
    let TunnResult::WriteToNetwork(init) = client_a.encapsulate(&[], &mut tx) else {
        panic!("expected post-reload handshake")
    };
    client_a_socket.send_to(init, address).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(3), client_a_socket.recv_from(&mut rx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()), 2);
    if let TunnResult::WriteToNetwork(packet) = client_a.decapsulate(None, &rx[..n], &mut tx) {
        client_a_socket.send_to(packet, address).await.unwrap();
    }
    let TunnResult::WriteToNetwork(wire) = client_a.encapsulate(&packet, &mut tx) else {
        panic!("expected encrypted packet")
    };
    client_a_socket.send_to(wire, address).await.unwrap();
    assert_no_inner(
        &client_b_migrated_socket,
        address,
        &mut client_b,
        &mut rx,
        &mut tx,
    )
    .await;

    let unknown = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let unknown_secret = StaticSecret::from([11u8; 32]);
    let mut unknown_client = Tunn::new(unknown_secret, hub_public, None, None, 35, None);
    let TunnResult::WriteToNetwork(init) = unknown_client.encapsulate(&[], &mut tx) else {
        panic!("expected initiation")
    };
    unknown.send_to(init, address).await.unwrap();
    assert!(
        timeout(Duration::from_millis(150), unknown.recv_from(&mut rx))
            .await
            .is_err(),
        "unknown static key must not receive a response"
    );
}

#[tokio::test]
async fn unrelated_reload_preserves_established_forward_tcp_tunnel_and_nat_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(Store::open(dir.path().join("cold-forward.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("10.88.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "clients".into(),
            name: "clients".into(),
            allowed_groups: vec!["backend".into()],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "backend".into(),
            name: "backend".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store.set_acl("clients", &["backend".into()]).unwrap();
    let a_secret = StaticSecret::from([81u8; 32]);
    let b_secret = StaticSecret::from([82u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &a_secret, "10.88.0.2", "clients"),
        ("b", &b_secret, "10.88.0.3", "backend"),
    ] {
        store
            .add_peer(&Peer {
                id: id.into(),
                name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(),
                group_id: group.into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    create_forward_for_test(
        &store,
        Forward {
            id: "tcp".into(),
            name: "tcp service".into(),
            protocol: "tcp".into(),
            target_peer_id: "b".into(),
            target_port: 5353,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    let hub_private = [83u8; 32];
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(kernel.run());
    let a_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret.clone(), hub_public, None, None, 81, None);
    let mut b = Tunn::new(b_secret.clone(), hub_public, None, None, 82, None);
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];
    establish_client(&a_socket, address, &mut a, &mut tx, &mut rx).await;
    establish_client(&b_socket, address, &mut b, &mut tx, &mut rx).await;

    let request = service_packet(
        6,
        [10, 88, 0, 2],
        [10, 88, 0, 1],
        12345,
        5353,
        0x02,
        b"syn-data",
    );
    send_inner(&a_socket, address, &mut a, &request, &mut tx).await;
    let delivered = recv_inner(&b_socket, &mut b, &mut rx, &mut tx).await;
    assert_eq!(&delivered[12..16], &[10, 88, 0, 1]);
    assert_eq!(&delivered[16..20], &[10, 88, 0, 3]);
    let translated_port = u16::from_be_bytes([delivered[20], delivered[21]]);
    assert_ne!(translated_port, 12345);
    assert_eq!(u16::from_be_bytes([delivered[22], delivered[23]]), 5353);
    assert_eq!(&delivered[40..], b"syn-data");
    assert_eq!(
        delivered[8], 63,
        "forward packet TTL is decremented exactly once"
    );
    assert_packet_checksums(&delivered);

    // Complete the backend side of the established stream before reload.
    let syn_ack = service_packet(
        6,
        [10, 88, 0, 3],
        [10, 88, 0, 1],
        5353,
        translated_port,
        0x12,
        b"syn-ack",
    );
    send_inner(&b_socket, address, &mut b, &syn_ack, &mut tx).await;
    let restored = recv_inner(&a_socket, &mut a, &mut rx, &mut tx).await;
    assert_eq!(&restored[12..16], &[10, 88, 0, 1]);
    assert_eq!(&restored[16..20], &[10, 88, 0, 2]);
    assert_eq!(u16::from_be_bytes([restored[20], restored[21]]), 5353);
    assert_eq!(u16::from_be_bytes([restored[22], restored[23]]), 12345);
    assert_eq!(&restored[40..], b"syn-ack");
    assert_packet_checksums(&restored);

    // Insert an unrelated group, then reload with this connection already
    // established. Keep the exact same client tunnel objects throughout.
    store
        .add_group(&Group {
            id: "unrelated".into(),
            name: "unrelated".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    handle.reload().await.unwrap();

    // Same client Tunn objects, established TCP tuple, and no fresh SYN or
    // handshake across an unrelated policy reload.
    let ack = service_packet(
        6,
        [10, 88, 0, 2],
        [10, 88, 0, 1],
        12345,
        5353,
        0x10,
        b"client-data",
    );
    send_inner(&a_socket, address, &mut a, &ack, &mut tx).await;
    let after_reload = recv_inner(&b_socket, &mut b, &mut rx, &mut tx).await;
    assert_eq!(
        u16::from_be_bytes([after_reload[20], after_reload[21]]),
        translated_port
    );
    assert_eq!(&after_reload[40..], b"client-data");
    assert_packet_checksums(&after_reload);
    let backend_data = service_packet(
        6,
        [10, 88, 0, 3],
        [10, 88, 0, 1],
        5353,
        translated_port,
        0x10,
        b"backend-data",
    );
    send_inner(&b_socket, address, &mut b, &backend_data, &mut tx).await;
    let restored = recv_inner(&a_socket, &mut a, &mut rx, &mut tx).await;
    assert_eq!(&restored[12..16], &[10, 88, 0, 1]);
    assert_eq!(&restored[16..20], &[10, 88, 0, 2]);
    assert_eq!(u16::from_be_bytes([restored[20], restored[21]]), 5353);
    assert_eq!(u16::from_be_bytes([restored[22], restored[23]]), 12345);
    assert_eq!(&restored[40..], b"backend-data");
    assert_packet_checksums(&restored);
    let snapshot = handle.stats().await;
    assert!(snapshot["a"].rx_bytes >= (request.len() + ack.len()) as u64);
    assert!(snapshot["b"].tx_bytes >= (delivered.len() + after_reload.len()) as u64);
}

#[tokio::test]
async fn cold_forward_two_syns_deliver_after_handshake_with_exact_reverse_mappings() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(Store::open(dir.path().join("cold-two-syns.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("10.88.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "clients".into(),
            name: "clients".into(),
            allowed_groups: vec!["backend".into()],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "backend".into(),
            name: "backend".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store.set_acl("clients", &["backend".into()]).unwrap();
    let a_secret = StaticSecret::from([91u8; 32]);
    let b_secret = StaticSecret::from([92u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &a_secret, "10.88.0.2", "clients"),
        ("b", &b_secret, "10.88.0.3", "backend"),
    ] {
        store
            .add_peer(&Peer {
                id: id.into(),
                name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(),
                group_id: group.into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    create_forward_for_test(
        &store,
        Forward {
            id: "tcp".into(),
            name: "tcp service".into(),
            protocol: "tcp".into(),
            target_peer_id: "b".into(),
            target_port: 5353,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    let hub_private = [93u8; 32];
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (queue_observer, mut observations) = mpsc::unbounded_channel();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(QUEUE_OBSERVER.scope(queue_observer, kernel.run()));
    let a_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret, hub_public, None, None, 91, None);
    let mut b = Tunn::new(b_secret, hub_public, None, None, 92, None);
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];
    establish_client(&a_socket, address, &mut a, &mut tx, &mut rx).await;

    let requests = [
        service_packet(
            6,
            [10, 88, 0, 2],
            [10, 88, 0, 1],
            12345,
            5353,
            0x02,
            b"first-cold-syn",
        ),
        service_packet(
            6,
            [10, 88, 0, 2],
            [10, 88, 0, 1],
            12346,
            5353,
            0x02,
            b"second-cold-syn",
        ),
    ];
    for request in &requests {
        send_inner(&a_socket, address, &mut a, request, &mut tx).await;
    }
    // Observe the actual application queue after each enqueue, not merely
    // the sender's UDP write or a later reload acknowledgement. This is the
    // barrier proving both SYNs are pending before policy is mutated.
    for expected_count in 1..=2 {
        let observed = timeout(Duration::from_secs(2), observations.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.queued.len(), expected_count);
        assert_eq!(
            observed
                .queued
                .iter()
                .map(
                    |(source, target, forward, src, dst, target_ip, port, proto)| (
                        source.as_str(),
                        target.as_str(),
                        forward.as_deref(),
                        *src,
                        *dst,
                        *target_ip,
                        *port,
                        *proto
                    )
                )
                .collect::<Vec<_>>(),
            requests[..expected_count]
                .iter()
                .map(|request| (
                    "a",
                    "b",
                    Some("tcp"),
                    Ipv4Addr::new(10, 88, 0, 2),
                    Ipv4Addr::new(10, 88, 0, 1),
                    Ipv4Addr::new(10, 88, 0, 3),
                    u16::from_be_bytes([request[20], request[21]]),
                    6
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            observed.queued_bytes,
            requests[..expected_count]
                .iter()
                .map(|request| request.len())
                .sum::<usize>()
        );
        assert_eq!(observed.flow_counts, (0, 0));
        assert_eq!(observed.source_counters, (0, 0));
    }
    let before_auth = handle.stats().await;
    assert_eq!(
        before_auth["a"].rx_bytes, 0,
        "cold forwarded SYNs are not committed before backend authentication"
    );
    assert_eq!(
        before_auth["b"].tx_bytes, 0,
        "no plaintext or byte accounting reaches an unauthenticated backend"
    );
    drop(before_auth);

    // Change unrelated persisted policy and apply it while both SYNs are
    // application-owned in the cold-target queue. Keep the tunnels intact.
    store
        .add_group(&Group {
            id: "unrelated".into(),
            name: "unrelated".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    handle.reload().await.unwrap();

    establish_client(&b_socket, address, &mut b, &mut tx, &mut rx).await;
    // An authenticated encrypted keepalive exercises the real UDP path and
    // makes the target's post-handshake readiness explicit.
    let TunnResult::WriteToNetwork(keepalive) = b.encapsulate(&[], &mut tx) else {
        panic!("expected authenticated keepalive")
    };
    b_socket.send_to(keepalive, address).await.unwrap();

    let mut mapped = Vec::new();
    for request in &requests {
        // The router may need to initiate its own session to B while
        // draining the pending packets. Complete that real peer-to-peer
        // WireGuard handshake response instead of treating it as app data.
        let delivered = loop {
            let (n, _) = timeout(Duration::from_secs(3), b_socket.recv_from(&mut rx))
                .await
                .unwrap()
                .unwrap();
            match b.decapsulate(None, &rx[..n], &mut tx) {
                TunnResult::WriteToTunnelV4(packet, _) => break packet.to_vec(),
                TunnResult::WriteToNetwork(packet) => {
                    b_socket.send_to(packet, address).await.unwrap();
                }
                _ => {}
            }
        };
        assert_eq!(&delivered[12..16], &[10, 88, 0, 1]);
        assert_eq!(&delivered[16..20], &[10, 88, 0, 3]);
        assert_eq!(u16::from_be_bytes([delivered[22], delivered[23]]), 5353);
        assert_eq!(delivered[8], 63, "SNAT/DNAT decrements TTL exactly once");
        assert_eq!(
            &delivered[40..],
            &request[40..],
            "each queued SYN payload is delivered unchanged"
        );
        assert_packet_checksums(&delivered);
        mapped.push((u16::from_be_bytes([delivered[20], delivered[21]]), request));
    }
    assert_ne!(
        mapped[0].0, mapped[1].0,
        "independent source tuples receive distinct SNAT ports"
    );
    assert_eq!(
        handle.stats().await["a"].rx_bytes,
        (requests[0].len() + requests[1].len()) as u64,
        "source accounting occurs only on actual delivery"
    );

    let mut replies = Vec::new();
    for (snat_port, request) in &mapped {
        let original_port = u16::from_be_bytes([request[20], request[21]]);
        let reply = service_packet(
            6,
            [10, 88, 0, 3],
            [10, 88, 0, 1],
            5353,
            *snat_port,
            0x12,
            b"syn-ack",
        );
        send_inner(&b_socket, address, &mut b, &reply, &mut tx).await;
        let restored = recv_inner(&a_socket, &mut a, &mut rx, &mut tx).await;
        assert_eq!(&restored[12..16], &[10, 88, 0, 1]);
        assert_eq!(&restored[16..20], &[10, 88, 0, 2]);
        assert_eq!(u16::from_be_bytes([restored[20], restored[21]]), 5353);
        assert_eq!(
            u16::from_be_bytes([restored[22], restored[23]]),
            original_port,
            "reverse mapping restores this SYN's exact original source port"
        );
        assert_eq!(&restored[40..], b"syn-ack");
        assert_packet_checksums(&restored);
        replies.push(reply);
    }
    assert_no_inner(&b_socket, address, &mut b, &mut rx, &mut tx).await;
    let snapshot = handle.stats().await;
    assert_eq!(
        snapshot["a"].rx_bytes,
        (requests[0].len() + requests[1].len()) as u64
    );
    assert_eq!(
        snapshot["b"].tx_bytes,
        (requests[0].len() + requests[1].len()) as u64
    );
    assert_eq!(
        snapshot["a"].tx_bytes,
        (replies[0].len() + replies[1].len()) as u64
    );
    assert_eq!(
        snapshot["b"].rx_bytes,
        (replies[0].len() + replies[1].len()) as u64
    );
}

#[tokio::test]
async fn cold_forward_deletion_ack_prevents_late_handshake_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        Store::open(
            dir.path()
                .join("cold-forward-delete.sqlite")
                .to_str()
                .unwrap(),
        )
        .unwrap(),
    );
    store.bind_test_identity();
    store
        .setup("10.88.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "clients".into(),
            name: "clients".into(),
            allowed_groups: vec!["backend".into()],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "backend".into(),
            name: "backend".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store.set_acl("clients", &["backend".into()]).unwrap();
    let a_secret = StaticSecret::from([101u8; 32]);
    let b_secret = StaticSecret::from([102u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &a_secret, "10.88.0.2", "clients"),
        ("b", &b_secret, "10.88.0.3", "backend"),
    ] {
        store
            .add_peer(&Peer {
                id: id.into(),
                name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(),
                group_id: group.into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    create_forward_for_test(
        &store,
        Forward {
            id: "tcp".into(),
            name: "tcp service".into(),
            protocol: "tcp".into(),
            target_peer_id: "b".into(),
            target_port: 5353,
            allowed_group_ids: vec!["clients".into()],
        },
    );
    let hub_private = [103u8; 32];
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (queue_observer, mut observations) = mpsc::unbounded_channel();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(QUEUE_OBSERVER.scope(queue_observer, kernel.run()));
    let a_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret, hub_public, None, None, 101, None);
    let mut b = Tunn::new(b_secret, hub_public, None, None, 102, None);
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];
    establish_client(&a_socket, address, &mut a, &mut tx, &mut rx).await;

    let queued_syn = service_packet(
        6,
        [10, 88, 0, 2],
        [10, 88, 0, 1],
        12345,
        5353,
        0x02,
        b"must-not-arrive",
    );
    send_inner(&a_socket, address, &mut a, &queued_syn, &mut tx).await;
    let observed = timeout(Duration::from_secs(2), observations.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        observed.queued,
        vec![(
            "a".into(),
            "b".into(),
            Some("tcp".into()),
            Ipv4Addr::new(10, 88, 0, 2),
            Ipv4Addr::new(10, 88, 0, 1),
            Ipv4Addr::new(10, 88, 0, 3),
            12345,
            6
        )]
    );
    assert_eq!(observed.queued_bytes, queued_syn.len());
    assert_eq!(observed.flow_counts, (0, 0));
    assert_eq!(observed.source_counters, (0, 0));
    assert_eq!(
        handle.stats().await["a"].rx_bytes,
        0,
        "cold forwarded SYN is not committed before backend authentication"
    );

    // Remove the actual persisted forward and wait for the router to publish
    // the new snapshot before B completes its first handshake.
    store.remove_forward("tcp").unwrap();
    handle.reload().await.unwrap();

    establish_client(&b_socket, address, &mut b, &mut tx, &mut rx).await;
    let TunnResult::WriteToNetwork(keepalive) = b.encapsulate(&[], &mut tx) else {
        panic!("expected authenticated keepalive")
    };
    b_socket.send_to(keepalive, address).await.unwrap();
    assert_no_inner(&b_socket, address, &mut b, &mut rx, &mut tx).await;
    assert_eq!(
        handle.stats().await["a"].rx_bytes,
        0,
        "acknowledged forward deletion discards the queued SYN without source accounting"
    );

    // The backend remains live and the directed ACL still authorizes direct
    // peer traffic after the forward-only policy change.
    let direct_syn = service_packet(
        6,
        [10, 88, 0, 2],
        [10, 88, 0, 3],
        23456,
        5353,
        0x02,
        b"direct-syn",
    );
    send_inner(&a_socket, address, &mut a, &direct_syn, &mut tx).await;
    let delivered = recv_inner(&b_socket, &mut b, &mut rx, &mut tx).await;
    assert_eq!(&delivered[12..16], &[10, 88, 0, 2]);
    assert_eq!(&delivered[16..20], &[10, 88, 0, 3]);
    assert_eq!(u16::from_be_bytes([delivered[20], delivered[21]]), 23456);
    assert_eq!(u16::from_be_bytes([delivered[22], delivered[23]]), 5353);
    assert_eq!(&delivered[40..], b"direct-syn");
    assert_packet_checksums(&delivered);

    let direct_reply = service_packet(
        6,
        [10, 88, 0, 3],
        [10, 88, 0, 2],
        5353,
        23456,
        0x12,
        b"direct-syn-ack",
    );
    send_inner(&b_socket, address, &mut b, &direct_reply, &mut tx).await;
    let restored = recv_inner(&a_socket, &mut a, &mut rx, &mut tx).await;
    assert_eq!(&restored[12..16], &[10, 88, 0, 3]);
    assert_eq!(&restored[16..20], &[10, 88, 0, 2]);
    assert_eq!(u16::from_be_bytes([restored[20], restored[21]]), 5353);
    assert_eq!(u16::from_be_bytes([restored[22], restored[23]]), 23456);
    assert_eq!(&restored[40..], b"direct-syn-ack");
    assert_packet_checksums(&restored);
}
#[tokio::test]
async fn invalid_mac_initiation_shapes_do_not_reach_anonymous_parse() {
    let store = Arc::new(Store::open(":memory:").unwrap());
    store.bind_test_identity();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(socket, store, [161u8; 32])
        .await
        .unwrap();
    let task = tokio::spawn(kernel.run());
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let before = ANON_PARSE_COUNT.with(|count| count.load(Ordering::SeqCst));
    let mut malformed = [0u8; 148];
    malformed[..4].copy_from_slice(&1u32.to_le_bytes());
    sender.send_to(&malformed, address).await.unwrap();
    time::sleep(Duration::from_millis(100)).await;
    let parsed = ANON_PARSE_COUNT.with(|count| count.load(Ordering::SeqCst)) - before;
    assert_eq!(
        parsed, 0,
        "invalid-MAC initiation must be rejected before anonymous parsing"
    );
    drop(handle);
    assert!(matches!(task.await.unwrap(), Err(RunError::Stopped)));
}

#[tokio::test]
async fn reload_revocation_blocks_established_direct_flow_requests_and_replies() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(Store::open(dir.path().join("cold-revoke.sqlite").to_str().unwrap()).unwrap());
    store.bind_test_identity();
    store
        .setup("10.89.0.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec!["b".into()],
        })
        .unwrap();
    store
        .add_group(&Group {
            id: "b".into(),
            name: "B".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    store.set_acl("a", &["b".into()]).unwrap();
    let a_secret = StaticSecret::from([91u8; 32]);
    let b_secret = StaticSecret::from([92u8; 32]);
    for (id, secret, ip, group) in [
        ("a", &a_secret, "10.89.0.2", "a"),
        ("b", &b_secret, "10.89.0.3", "b"),
    ] {
        store
            .add_peer(&Peer {
                id: id.into(),
                name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(),
                group_id: group.into(),
                received_bytes: 0,
                sent_bytes: 0,
                last_handshake_unix: None,
            })
            .unwrap();
    }
    let hub_private = [93u8; 32];
    let hub_public = PublicKey::from(&StaticSecret::from(hub_private));
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (kernel, handle) = initialize_store_runtime(server, store.clone(), hub_private)
        .await
        .unwrap();
    tokio::spawn(kernel.run());
    let a_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut a = Tunn::new(a_secret.clone(), hub_public, None, None, 91, None);
    let mut b = Tunn::new(b_secret.clone(), hub_public, None, None, 92, None);
    let mut tx = vec![0; 65535];
    let mut rx = vec![0; 65535];
    establish_client(&a_socket, address, &mut a, &mut tx, &mut rx).await;
    establish_client(&b_socket, address, &mut b, &mut tx, &mut rx).await;
    let request = service_packet(
        17,
        [10, 89, 0, 2],
        [10, 89, 0, 3],
        2345,
        9090,
        0,
        b"before-revoke",
    );
    send_inner(&a_socket, address, &mut a, &request, &mut tx).await;
    assert_eq!(
        &recv_inner(&b_socket, &mut b, &mut rx, &mut tx).await[28..],
        b"before-revoke"
    );

    // Revocation removes established state before acknowledging policy.
    store.set_acl("a", &[]).unwrap();
    handle.reload().await.unwrap();
    let reply = service_packet(
        17,
        [10, 89, 0, 3],
        [10, 89, 0, 2],
        9090,
        2345,
        0,
        b"revoked-reply",
    );
    send_inner(&b_socket, address, &mut b, &reply, &mut tx).await;
    assert_no_inner(&a_socket, address, &mut a, &mut rx, &mut tx).await;
    send_inner(
        &a_socket,
        address,
        &mut a,
        &service_packet(
            17,
            [10, 89, 0, 2],
            [10, 89, 0, 3],
            2345,
            9090,
            0,
            b"after-revoke",
        ),
        &mut tx,
    )
    .await;
    assert_no_inner(&b_socket, address, &mut b, &mut rx, &mut tx).await;
    let snapshot = handle.stats().await;
    assert_eq!(
        snapshot["a"].rx_bytes,
        request.len() as u64,
        "revoked traffic is not counted as delivered ingress"
    );
    assert_eq!(
        snapshot["b"].tx_bytes,
        request.len() as u64,
        "revoked traffic is not counted as delivered egress"
    );
}
