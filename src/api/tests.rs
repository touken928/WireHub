use super::*;
use std::sync::Arc;
use axum::{extract::{Path, State}, http::{header, HeaderMap, StatusCode}, response::IntoResponse, Json};
use crate::{storage::Store, transport::{Readiness, ReloadCommand, RuntimeStats}};
    fn test_state(configured:bool)->(AppState,tokio::sync::mpsc::Receiver<ReloadCommand>,HeaderMap){
        let store=Arc::new(Store::open(":memory:").unwrap());store.bind_test_identity();if configured{store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();}
        let (reload_tx,reload_rx)=tokio::sync::mpsc::channel(8);let state=AppState{store,token:Some("secret".into()),hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default(),readiness:Readiness::default()};let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());(state,reload_rx,headers)
    }
    async fn response_text(response:axum::response::Response)->(StatusCode,String){let status=response.status();let bytes=axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap();(status,String::from_utf8_lossy(&bytes).into_owned())}
    #[test]
    fn authentication_fails_closed_for_missing_or_empty_token() {
        let (reload_tx,_) = tokio::sync::mpsc::channel(1);let state=AppState{store:Arc::new(Store::open(":memory:").unwrap()),token:None,hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default(),readiness:Readiness::default()};
        assert!(!auth(&HeaderMap::new(),&state));
        let state=AppState{token:Some(String::new()),..state};assert!(!auth(&HeaderMap::new(),&state));
    }
    #[test]
    fn authentication_requires_exact_bearer_token() {
        let (reload_tx,_) = tokio::sync::mpsc::channel(1);let state=AppState{store:Arc::new(Store::open(":memory:").unwrap()),token:Some("secret".into()),hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default(),readiness:Readiness::default()};
        assert!(!auth(&HeaderMap::new(),&state));let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());assert!(auth(&headers,&state));
    }
    #[tokio::test]
    async fn readiness_is_separate_from_compatible_liveness_health() {
        let (state,_,_)=test_state(false);
        let (status,body)=response_text(health().await.into_response()).await;
        assert_eq!(status,StatusCode::OK);assert!(body.contains("\"ok\":true"));
        let (status,_)=response_text(ready(State(state.clone())).await.into_response()).await;
        assert_eq!(status,StatusCode::SERVICE_UNAVAILABLE);
        state.readiness.set(true);
        let (status,body)=response_text(ready(State(state)).await.into_response()).await;
        assert_eq!(status,StatusCode::OK);assert!(body.contains("\"ok\":true"));
    }
    #[tokio::test]
    async fn provisioning_uses_standard_wireguard_base64_without_persisting_private_key() {
        use base64::Engine;
        let store=Arc::new(Store::open(":memory:").unwrap());
        store.bind_test_identity();store.setup("192.168.44.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"g".into(),name:"group".into(),allowed_groups:vec![]}).unwrap();
        let (reload_tx,mut reload_rx)=tokio::sync::mpsc::channel::<ReloadCommand>(1);tokio::spawn(async move{if let Some(command)=reload_rx.recv().await{let _=command.ack.send(Ok(()));}});
        let state=AppState{store:store.clone(),token:Some("secret".into()),hub_public:"hub-public".into(),reload_tx,runtime_stats:RuntimeStats::default(),readiness:Readiness::default()};
        let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());
        let response=create_peer(State(state),headers,Json(NewPeer{name:"client".into(),group_id:"g".into()})).await.into_response();
        assert_eq!(response.status(),StatusCode::CREATED);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(),"no-store");
        let body=axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap();
        #[derive(serde::Deserialize)] struct ProvisionBody { peer:Peer, config:String }
        let provision:ProvisionBody=serde_json::from_slice(&body).unwrap();
        assert_eq!(provision.peer.ipv4,"192.168.44.2");
        assert!(provision.config.contains("Address = 192.168.44.2/32\n"));
        assert!(provision.config.contains("Endpoint = hub.example:51820\n"));
        assert!(provision.config.contains("AllowedIPs = 192.168.44.0/24\n"));
        assert!(provision.config.contains("PersistentKeepalive = 25\n"));
        assert_eq!(provision.config.lines().filter(|line|line.starts_with("AllowedIPs = ")).count(),1);
        let encoded_public=provision.peer.public_key.clone();
        let public_bytes=base64::engine::general_purpose::STANDARD.decode(&encoded_public).unwrap();
        assert_eq!(public_bytes.len(),32);
        let stored=store.peers().unwrap();
        assert_eq!(stored.len(),1);
        assert_eq!(stored[0].public_key,encoded_public);
        let private_key=provision.config.lines().find_map(|line|line.strip_prefix("PrivateKey = ")).unwrap();
        let private_bytes=base64::engine::general_purpose::STANDARD.decode(private_key).unwrap();
        assert_eq!(private_bytes.len(),32);
        assert!(!serde_json::to_string(&stored).unwrap().contains(private_key));
        assert!(!stored[0].public_key.contains(private_key));
    }
    #[tokio::test]
    async fn failed_setup_activation_reports_persisted_settings() {
        let store=Arc::new(Store::open(":memory:").unwrap());
        let (reload_tx,mut reload_rx)=tokio::sync::mpsc::channel::<ReloadCommand>(1);
        tokio::spawn(async move { if let Some(command)=reload_rx.recv().await { let _=command.ack.send(Err(())); } });
        store.bind_test_identity();let state=AppState{store:store.clone(),token:Some("secret".into()),hub_public:String::new(),reload_tx,runtime_stats:RuntimeStats::default(),readiness:Readiness::default()};
        let mut headers=HeaderMap::new();headers.insert("authorization","Bearer secret".parse().unwrap());
        let response=post_setup(State(state),headers,Json(SetupRequest{subnet:"172.23.45.0/24".into(),endpoint:"hub.example:51820".into(),persistent_keepalive:25})).await.into_response();
        assert_eq!(response.status(),StatusCode::SERVICE_UNAVAILABLE);
        let body=axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("settings saved"));
        assert_eq!(store.network_settings().unwrap().unwrap().subnet,"172.23.45.0/24");
    }
    #[tokio::test]
    async fn failed_peer_activation_removes_peer_and_waits_for_cleanup_reload() {
        let (state, mut reload_rx, headers) = test_state(true);
        state.store.add_group(&Group { id: "g".into(), name: "group".into(), allowed_groups: vec![] }).unwrap();
        let worker = tokio::spawn(async move {
            let first = reload_rx.recv().await.unwrap();
            let _ = first.ack.send(Err(()));
            let cleanup = reload_rx.recv().await.unwrap();
            let _ = cleanup.ack.send(Ok(()));
        });
        let response = create_peer(
            State(state.clone()), headers,
            Json(NewPeer { name: "client".into(), group_id: "g".into() }),
        ).await.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
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
    async fn endpoint_validation_status_matrix_does_not_queue_reload_for_failures(){
        let (s,mut rx,h)=test_state(false);
        let f=|name:&str,target:&str,groups:Vec<String>,port|NewForward{name:name.into(),protocol:"tcp".into(),target_peer_id:target.into(),target_port:port,allowed_group_ids:groups};
        let(status,detail)=response_text(create_forward(State(s.clone()),h.clone(),Json(f("f","missing",vec![],80))).await.into_response()).await;assert_eq!(status,StatusCode::CONFLICT);assert!(detail.contains("setup required"));
        assert!(rx.try_recv().is_err());
        s.store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();s.store.add_group(&Group{id:"g".into(),name:"group".into(),allowed_groups:vec![]}).unwrap();let mut p=Peer{id:"p".into(),name:"peer".into(),public_key:"pk".into(),ipv4:String::new(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};assert!(s.store.create_peer_allocated(&mut p).unwrap());
        for (request,expected,detail) in [(f("bad","p",vec!["missing".into()],81),StatusCode::BAD_REQUEST,"invalid reference"),(f("bad","missing",vec![],82),StatusCode::BAD_REQUEST,"invalid reference"),(f("   ","p",vec![],83),StatusCode::BAD_REQUEST,"name must not be empty")]{let(status,body)=response_text(create_forward(State(s.clone()),h.clone(),Json(request)).await.into_response()).await;assert_eq!(status,expected);assert!(body.contains(detail),"{body}");assert!(rx.try_recv().is_err());}
        let first=f("dupe","p",vec!["g".into()],84);s.store.create_forward(&mut Forward{id:"first".into(),name:first.name.clone(),protocol:first.protocol.clone(),target_peer_id:first.target_peer_id.clone(),target_port:first.target_port,allowed_group_ids:first.allowed_group_ids.clone()}).unwrap();let(status,body)=response_text(create_forward(State(s.clone()),h.clone(),Json(f("dupe2","p",vec!["g".into()],84))).await.into_response()).await;assert_eq!(status,StatusCode::CONFLICT);assert!(body.contains("forward conflict"));assert!(rx.try_recv().is_err());
        let(status,body)=response_text(set_acl(State(s.clone()),h.clone(),Path("absent".into()),Json(SetAcl{allowed_groups:vec!["g".into()]})).await.into_response()).await;assert_eq!(status,StatusCode::NOT_FOUND);assert!(body.contains("group not found"));
        let(status,body)=response_text(delete_forward(State(s.clone()),h.clone(),Path("absent".into())).await.into_response()).await;assert_eq!(status,StatusCode::NOT_FOUND);assert!(body.contains("forward not found"));
        let(status,body)=response_text(delete_group(State(s.clone()),h.clone(),Path("g".into())).await.into_response()).await;assert_eq!(status,StatusCode::CONFLICT);assert!(body.contains("resource is in use"));assert!(rx.try_recv().is_err());
        let unchanged=s.store.forwards().unwrap();assert_eq!(unchanged.len(),1);assert_eq!(unchanged[0].allowed_group_ids,vec!["g"]);assert_eq!(s.store.group("g").unwrap().unwrap().allowed_groups,Vec::<String>::new());
    }
