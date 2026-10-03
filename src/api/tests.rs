use super::*;
use crate::{
    kernel::{
        control::{test_harness, TestReceiver},
        ReloadError,
    },
    storage::Store,
};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use std::time::Duration;
fn test_state(configured: bool) -> (AppState, TestReceiver, HeaderMap) {
    let store = Arc::new(Store::open(":memory:").unwrap());
    store.bind_test_identity();
    if configured {
        store
            .setup("10.88.0.0/24", "hub.example:51820", 25)
            .unwrap();
    }
    let (kernel, reload_rx) = test_harness();
    let state = AppState {
        store,
        token: Some("secret".into()),
        hub_public: String::new(),
        kernel,
    };
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer secret".parse().unwrap());
    (state, reload_rx, headers)
}
async fn response_text(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
async fn assert_no_reload(rx: &mut TestReceiver) {
    assert!(
        tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .is_err(),
        "validation failure queued reload"
    );
}
#[test]
fn authentication_fails_closed_for_missing_or_empty_token() {
    let (kernel, _) = test_harness();
    let state = AppState {
        store: Arc::new(Store::open(":memory:").unwrap()),
        token: None,
        hub_public: String::new(),
        kernel,
    };
    assert!(!auth(&HeaderMap::new(), &state));
    let state = AppState {
        token: Some(String::new()),
        ..state
    };
    assert!(!auth(&HeaderMap::new(), &state));
}
#[test]
fn authentication_requires_exact_bearer_token() {
    let (kernel, _) = test_harness();
    let state = AppState {
        store: Arc::new(Store::open(":memory:").unwrap()),
        token: Some("secret".into()),
        hub_public: String::new(),
        kernel,
    };
    assert!(!auth(&HeaderMap::new(), &state));
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer secret".parse().unwrap());
    assert!(auth(&headers, &state));
}
#[tokio::test]
async fn readiness_is_separate_from_compatible_liveness_health() {
    let store = Arc::new(Store::open(":memory:").unwrap());
    store.bind_test_identity();
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (kernel, handle) = crate::kernel::Kernel::initialize(socket, [0; 32], || {
        Ok(
            crate::kernel::CompiledSnapshot::try_from(crate::model::NetworkSnapshot {
                settings: None,
                groups: vec![],
                peers: vec![],
                forwards: vec![],
            })
            .unwrap(),
        )
    })
    .await
    .unwrap();
    let state = AppState {
        store,
        token: Some("secret".into()),
        hub_public: String::new(),
        kernel: handle,
    };
    let (status, body) = response_text(health().await.into_response()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"ok\":true"));
    let (status, _) = response_text(ready(State(state.clone())).await.into_response()).await;
    assert_eq!(status, StatusCode::OK);
    drop(kernel);
    let (status, _) = response_text(ready(State(state)).await.into_response()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
#[tokio::test]
async fn provisioning_uses_standard_wireguard_base64_without_persisting_private_key() {
    use base64::Engine;
    let store = Arc::new(Store::open(":memory:").unwrap());
    store.bind_test_identity();
    store
        .setup("192.168.44.0/24", "hub.example:51820", 25)
        .unwrap();
    store
        .add_group(&Group {
            id: "g".into(),
            name: "group".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let (kernel, mut reload_rx) = test_harness();
    tokio::spawn(async move {
        if let Some(command) = reload_rx.recv().await {
            command.respond(Ok(()));
        }
    });
    let state = AppState {
        store: store.clone(),
        token: Some("secret".into()),
        hub_public: "hub-public".into(),
        kernel,
    };
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer secret".parse().unwrap());
    let response = create_peer(
        State(state),
        headers,
        Json(NewPeer {
            name: "client".into(),
            group_id: "g".into(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    #[derive(serde::Deserialize)]
    struct ProvisionBody {
        peer: Peer,
        config: String,
    }
    let provision: ProvisionBody = serde_json::from_slice(&body).unwrap();
    assert_eq!(provision.peer.ipv4, "192.168.44.2");
    assert!(provision.config.contains("Address = 192.168.44.2/32\n"));
    assert!(provision.config.contains("Endpoint = hub.example:51820\n"));
    assert!(provision.config.contains("AllowedIPs = 192.168.44.0/24\n"));
    assert!(provision.config.contains("PersistentKeepalive = 25\n"));
    assert_eq!(
        provision
            .config
            .lines()
            .filter(|line| line.starts_with("AllowedIPs = "))
            .count(),
        1
    );
    let encoded_public = provision.peer.public_key.clone();
    let public_bytes = base64::engine::general_purpose::STANDARD
        .decode(&encoded_public)
        .unwrap();
    assert_eq!(public_bytes.len(), 32);
    let stored = store.peers().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].public_key, encoded_public);
    let private_key = provision
        .config
        .lines()
        .find_map(|line| line.strip_prefix("PrivateKey = "))
        .unwrap();
    let private_bytes = base64::engine::general_purpose::STANDARD
        .decode(private_key)
        .unwrap();
    assert_eq!(private_bytes.len(), 32);
    assert!(!serde_json::to_string(&stored)
        .unwrap()
        .contains(private_key));
    assert!(!stored[0].public_key.contains(private_key));
}
#[tokio::test]
async fn failed_setup_activation_reports_persisted_settings() {
    let store = Arc::new(Store::open(":memory:").unwrap());
    let (kernel, mut reload_rx) = test_harness();
    tokio::spawn(async move {
        if let Some(command) = reload_rx.recv().await {
            command.respond(Err(ReloadError::Rejected));
        }
    });
    store.bind_test_identity();
    let state = AppState {
        store: store.clone(),
        token: Some("secret".into()),
        hub_public: String::new(),
        kernel,
    };
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer secret".parse().unwrap());
    let response = post_setup(
        State(state),
        headers,
        Json(SetupRequest {
            subnet: "172.23.45.0/24".into(),
            endpoint: "hub.example:51820".into(),
            persistent_keepalive: 25,
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("settings saved"));
    assert_eq!(
        store.network_settings().unwrap().unwrap().subnet,
        "172.23.45.0/24"
    );
}
#[tokio::test]
async fn failed_peer_activation_removes_peer_and_waits_for_cleanup_reload() {
    let (state, mut reload_rx, headers) = test_state(true);
    state
        .store
        .add_group(&Group {
            id: "g".into(),
            name: "group".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let worker = tokio::spawn(async move {
        let first = reload_rx.recv().await.unwrap();
        first.respond(Err(ReloadError::Rejected));
        let cleanup = reload_rx.recv().await.unwrap();
        cleanup.respond(Ok(()));
    });
    let response = create_peer(
        State(state.clone()),
        headers,
        Json(NewPeer {
            name: "client".into(),
            group_id: "g".into(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("peer removal and cleanup reload acknowledged"));
    assert!(state.store.peers().unwrap().is_empty());
    worker.await.unwrap();
}

#[tokio::test]
async fn reload_fails_when_command_receiver_is_closed() {
    let (state, receiver, _) = test_state(false);
    drop(receiver);
    assert!(!reload(&state).await);
}
#[tokio::test]
async fn endpoint_validation_status_matrix_does_not_queue_reload_for_failures() {
    let (s, mut rx, h) = test_state(false);
    let f = |name: &str, target: &str, groups: Vec<String>, port| NewForward {
        name: name.into(),
        protocol: "tcp".into(),
        target_peer_id: target.into(),
        target_port: port,
        allowed_group_ids: groups,
    };
    let (status, detail) = response_text(
        create_forward(
            State(s.clone()),
            h.clone(),
            Json(f("f", "missing", vec![], 80)),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(detail.contains("setup required"));
    assert_no_reload(&mut rx).await;
    s.store
        .setup("10.88.0.0/24", "hub.example:51820", 25)
        .unwrap();
    s.store
        .add_group(&Group {
            id: "g".into(),
            name: "group".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let mut p = Peer {
        id: "p".into(),
        name: "peer".into(),
        public_key: "pk".into(),
        ipv4: String::new(),
        group_id: "g".into(),
        received_bytes: 0,
        sent_bytes: 0,
        last_handshake_unix: None,
    };
    assert!(s.store.create_peer_allocated(&mut p).unwrap());
    for (request, expected, detail) in [
        (
            f("bad", "p", vec!["missing".into()], 81),
            StatusCode::BAD_REQUEST,
            "invalid reference",
        ),
        (
            f("bad", "missing", vec![], 82),
            StatusCode::BAD_REQUEST,
            "invalid reference",
        ),
        (
            f("   ", "p", vec![], 83),
            StatusCode::BAD_REQUEST,
            "name must not be empty",
        ),
    ] {
        let (status, body) = response_text(
            create_forward(State(s.clone()), h.clone(), Json(request))
                .await
                .into_response(),
        )
        .await;
        assert_eq!(status, expected);
        assert!(body.contains(detail), "{body}");
        assert_no_reload(&mut rx).await;
    }
    let first = f("dupe", "p", vec!["g".into()], 84);
    s.store
        .create_forward(&mut Forward {
            id: "first".into(),
            name: first.name.clone(),
            protocol: first.protocol.clone(),
            target_peer_id: first.target_peer_id.clone(),
            target_port: first.target_port,
            allowed_group_ids: first.allowed_group_ids.clone(),
        })
        .unwrap();
    let (status, body) = response_text(
        create_forward(
            State(s.clone()),
            h.clone(),
            Json(f("dupe2", "p", vec!["g".into()], 84)),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("forward conflict"));
    assert_no_reload(&mut rx).await;
    let (status, body) = response_text(
        set_acl(
            State(s.clone()),
            h.clone(),
            Path("absent".into()),
            Json(SetAcl {
                allowed_groups: vec!["g".into()],
            }),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("group not found"));
    let (status, body) = response_text(
        delete_forward(State(s.clone()), h.clone(), Path("absent".into()))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("forward not found"));
    let (status, body) = response_text(
        delete_group(State(s.clone()), h.clone(), Path("g".into()))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("resource is in use"));
    assert_no_reload(&mut rx).await;
    let unchanged = s.store.forwards().unwrap();
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0].allowed_group_ids, vec!["g"]);
    assert_eq!(
        s.store.group("g").unwrap().unwrap().allowed_groups,
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn http_disconnect_during_peer_activation_cleans_up_and_allows_retry() {
    use tokio::io::AsyncWriteExt;
    let (state, mut rx, _) = test_state(true);
    state
        .store
        .add_group(&Group {
            id: "g".into(),
            name: "g".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    let body = r#"{"name":"disconnected","group_id":"g"}"#;
    let request=format!("POST /api/peers HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",body.len(),body);
    client.write_all(request.as_bytes()).await.unwrap();
    let _command = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.store.runtime_snapshot().unwrap().peers.len(), 1);
    assert!(
        state.store.peers().unwrap().is_empty(),
        "pending provision is hidden from inventory"
    );
    drop(client);
    let cleanup = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(state.store.runtime_snapshot().unwrap().peers.is_empty());
    cleanup.respond(Ok(()));
    let worker = tokio::spawn(async move {
        rx.recv().await.unwrap().respond(Ok(()));
    });
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer secret".parse().unwrap());
    let response = create_peer(
        State(state.clone()),
        headers,
        Json(NewPeer {
            name: "disconnected".into(),
            group_id: "g".into(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(state.store.peers().unwrap()[0].ipv4, "10.88.0.2");
    worker.await.unwrap();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn concurrent_peer_deletion_during_activation_never_returns_unusable_config() {
    let (state, mut rx, headers) = test_state(true);
    state
        .store
        .add_group(&Group {
            id: "g".into(),
            name: "g".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    let task = tokio::spawn(create_peer(
        State(state.clone()),
        headers,
        Json(NewPeer {
            name: "deleted".into(),
            group_id: "g".into(),
        }),
    ));
    let command = rx.recv().await.unwrap();
    let id = state.store.runtime_snapshot().unwrap().peers[0].id.clone();
    state.store.remove_peer(&id).unwrap();
    command.respond(Ok(()));
    let cleanup = rx.recv().await.unwrap();
    cleanup.respond(Ok(()));
    let response = task.await.unwrap().into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("PrivateKey"));
    assert!(state.store.runtime_snapshot().unwrap().peers.is_empty());
}

#[tokio::test]
async fn cancel_acl_update_at_cooperative_reservation_leaves_database_unchanged() {
    use std::{future::{poll_fn, Future}, task::Poll};

    let (state, mut rx, headers) = test_state(true);
    for id in ["source", "allowed"] {
        state
            .store
            .add_group(&Group {
                id: id.into(),
                name: id.into(),
                allowed_groups: vec![],
            })
            .unwrap();
    }

    // Exhaust this poll's budget without yielding to the scheduler. The next
    // cooperative operation must therefore suspend before it can enqueue.
    poll_fn(|cx| loop {
        let mut consume = Box::pin(tokio::task::consume_budget());
        if consume.as_mut().poll(cx).is_pending() {
            return Poll::Ready(());
        }
    })
    .await;

    let mut request = Box::pin(set_acl(
        State(state.clone()),
        headers,
        Path("source".into()),
        Json(SetAcl {
            allowed_groups: vec!["allowed".into()],
        }),
    ));
    let suspended_before_completion = poll_fn(|cx| {
        Poll::Ready(request.as_mut().poll(cx).is_pending())
    })
    .await;
    drop(request);

    assert!(suspended_before_completion, "handler unexpectedly completed");
    assert_eq!(
        state
            .store
            .group("source")
            .unwrap()
            .unwrap()
            .allowed_groups,
        Vec::<String>::new(),
        "cancelled request must not commit before obtaining a reload permit"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .is_err(),
        "reload should not have been enqueued before cancellation"
    );
}

#[tokio::test]
async fn cancel_acl_update_after_sync_send_keeps_reload_command_queued() {
    let (state, mut rx, headers) = test_state(true);
    for id in ["source", "other"] {
        state
            .store
            .add_group(&Group {
                id: id.into(),
                name: id.into(),
                allowed_groups: vec![],
            })
            .unwrap();
    }
    let task = tokio::spawn(set_acl(
        State(state.clone()),
        headers,
        Path("source".into()),
        Json(SetAcl {
            allowed_groups: vec!["other".into()],
        }),
    ));
    let command = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        state
            .store
            .group("source")
            .unwrap()
            .unwrap()
            .allowed_groups,
        vec!["other"]
    );
    task.abort();
    let _ = task.await;
    // The reload command has already been synchronously published. Its ack
    // receiver may be gone, but cancellation cannot retract the command.
    command.respond(Ok(()));
    assert_no_reload(&mut rx).await;
}

#[tokio::test]
async fn no_op_and_database_error_release_reserved_reload_capacity() {
    let (state, mut rx, headers) = test_state(true);
    for id in ["source", "other"] {
        state
            .store
            .add_group(&Group {
                id: id.into(),
                name: id.into(),
                allowed_groups: vec![],
            })
            .unwrap();
    }
    let missing = set_acl(
        State(state.clone()),
        headers.clone(),
        Path("missing".into()),
        Json(SetAcl { allowed_groups: vec![] }),
    )
    .await
    .into_response();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let invalid = set_acl(
        State(state.clone()),
        headers.clone(),
        Path("source".into()),
        Json(SetAcl {
            allowed_groups: vec!["missing".into()],
        }),
    )
    .await
    .into_response();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_no_reload(&mut rx).await;

    // Hold every published slot at once. This catches even small permit leaks
    // that would leave enough capacity for one subsequent valid request.
    let mut permits = Vec::with_capacity(16);
    for _ in 0..16 {
        permits.push(
            tokio::time::timeout(Duration::from_millis(100), state.kernel.reserve_reload())
                .await
                .expect("available reload capacity should be reservable")
                .expect("reload channel should remain open"),
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), state.kernel.reserve_reload())
            .await
            .is_err(),
        "all sixteen reload slots should be held"
    );
    drop(permits);

    let task = tokio::spawn(set_acl(
        State(state),
        headers,
        Path("source".into()),
        Json(SetAcl {
            allowed_groups: vec!["other".into()],
        }),
    ));
    let command = rx.recv().await.unwrap();
    command.respond(Ok(()));
    assert_eq!(task.await.unwrap().into_response().status(), StatusCode::OK);
}

#[tokio::test]
async fn full_reload_queue_rejects_acl_and_setup_before_database_mutation() {
    let (state, mut rx, headers) = test_state(true);
    state
        .store
        .add_group(&Group {
            id: "source".into(),
            name: "source".into(),
            allowed_groups: vec![],
        })
        .unwrap();
    for _ in 0..16 {
        drop(state.kernel.reserve_reload().await.unwrap().send());
    }

    let (status, _) = response_text(
        set_acl(
            State(state.clone()),
            headers.clone(),
            Path("source".into()),
            Json(SetAcl {
                allowed_groups: vec!["another".into()],
            }),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(state
        .store
        .group("source")
        .unwrap()
        .unwrap()
        .allowed_groups
        .is_empty());

    let mut unconfigured = test_state(false);
    for _ in 0..16 {
        drop(unconfigured.0.kernel.reserve_reload().await.unwrap().send());
    }
    let (status, body) = response_text(
        post_setup(
            State(unconfigured.0.clone()),
            unconfigured.2,
            Json(SetupRequest {
                subnet: "172.23.45.0/24".into(),
                endpoint: "hub.example:51820".into(),
                persistent_keepalive: 25,
            }),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(!body.contains("settings saved"));
    assert!(unconfigured.0.store.network_settings().unwrap().is_none());

    for _ in 0..16 {
        assert!(rx.recv().await.is_some());
    }
    assert_no_reload(&mut rx).await;
    for _ in 0..16 {
        assert!(unconfigured.1.recv().await.is_some());
    }
    assert_no_reload(&mut unconfigured.1).await;
}

#[tokio::test]
async fn full_reload_queue_rejects_each_mutation_family_before_writing() {
    let (state, mut rx, headers) = test_state(true);
    for id in ["g", "other"] {
        state
            .store
            .add_group(&Group {
                id: id.into(),
                name: id.into(),
                allowed_groups: vec![],
            })
            .unwrap();
    }
    let mut peer = Peer {
        id: "p".into(),
        name: "peer".into(),
        public_key: "pk".into(),
        ipv4: String::new(),
        group_id: "g".into(),
        received_bytes: 0,
        sent_bytes: 0,
        last_handshake_unix: None,
    };
    assert!(state.store.create_peer_allocated(&mut peer).unwrap());
    state
        .store
        .create_forward(&mut Forward {
            id: "f".into(),
            name: "forward".into(),
            protocol: "tcp".into(),
            target_peer_id: "p".into(),
            target_port: 80,
            allowed_group_ids: vec!["g".into()],
        })
        .unwrap();
    let before = state.store.runtime_snapshot().unwrap();
    for _ in 0..16 {
        drop(state.kernel.reserve_reload().await.unwrap().send());
    }

    let (create_group_status, delete_group_status, acl_status, create_forward_status,
        delete_forward_status, create_peer_status, delete_peer_status, move_peer_status) = tokio::join!(
        async { create_group(State(state.clone()), headers.clone(), Json(NewGroup { name: "new".into() })).await.into_response().status() },
        async { delete_group(State(state.clone()), headers.clone(), Path("other".into())).await.into_response().status() },
        async { set_acl(State(state.clone()), headers.clone(), Path("g".into()), Json(SetAcl { allowed_groups: vec!["other".into()] })).await.into_response().status() },
        async { create_forward(State(state.clone()), headers.clone(), Json(NewForward { name: "new".into(), protocol: "tcp".into(), target_peer_id: "p".into(), target_port: 81, allowed_group_ids: vec!["g".into()] })).await.into_response().status() },
        async { delete_forward(State(state.clone()), headers.clone(), Path("f".into())).await.into_response().status() },
        async { create_peer(State(state.clone()), headers.clone(), Json(NewPeer { name: "new".into(), group_id: "g".into() })).await.into_response().status() },
        async { delete_peer(State(state.clone()), headers.clone(), Path("p".into())).await.into_response().status() },
        async { move_peer(State(state.clone()), headers.clone(), Path("p".into()), Json(MovePeer { group_id: "other".into() })).await.into_response().status() },
    );
    assert_eq!(
        [create_group_status, delete_group_status, acl_status, create_forward_status,
            delete_forward_status, create_peer_status, delete_peer_status, move_peer_status],
        [StatusCode::SERVICE_UNAVAILABLE; 8]
    );
    let after = state.store.runtime_snapshot().unwrap();
    assert_eq!(serde_json::to_value(after.settings).unwrap(), serde_json::to_value(before.settings).unwrap());
    assert_eq!(serde_json::to_value(after.groups).unwrap(), serde_json::to_value(before.groups).unwrap());
    assert_eq!(serde_json::to_value(after.peers).unwrap(), serde_json::to_value(before.peers).unwrap());
    assert_eq!(serde_json::to_value(after.forwards).unwrap(), serde_json::to_value(before.forwards).unwrap());
    for _ in 0..16 {
        assert!(rx.recv().await.is_some());
    }
    assert_no_reload(&mut rx).await;
}
