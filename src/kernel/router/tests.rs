use super::*;
use crate::kernel::checksum::checksum;
use super::delivery::*;
use super::snapshot::*;
    use crate::model::Group;
    use boringtun::x25519::StaticSecret;
    use tokio::time::timeout;
    use base64::Engine;
    fn create_forward_for_test(store:&Store,mut forward:Forward){store.create_forward(&mut forward).unwrap();}

    fn pending_packet(payload_len: usize) -> ipv4::ValidatedPacket {
        let raw=service_packet(17,[10,77,0,2],[10,77,0,3],1234,5678,0,&vec![0;payload_len]);
        let source=Peer{id:"a".into(),name:"a".into(),public_key:String::new(),ipv4:"10.77.0.2".into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let group=Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]};
        ipv4::validate(&raw,&source,Some(&group)).unwrap()
    }

    #[test]
    fn app_pending_queue_enforces_entry_byte_and_deadline_bounds() {
        let now=Instant::now();
        let packet=pending_packet(0);
        let mut queue=VecDeque::new(); let mut bytes=0;
        for _ in 0..=PENDING_LIMIT {
            enqueue_pending(&mut queue,&mut bytes,PendingDelivery{source_id:"a".into(),source_key:String::new(),source_ip:packet.src(),packet:packet.clone(),reply_only:false,deadline:now+PENDING_TTL,target_id:"b".into(),target_key:String::new(),target_ip:Ipv4Addr::UNSPECIFIED,forward_id:None,forward_protocol:None,forward_target_port:None});
        }
        assert_eq!(queue.len(),PENDING_LIMIT);
        assert_eq!(bytes,PENDING_LIMIT*packet.bytes().len());

        let large=pending_packet(60_000);
        let mut queue=VecDeque::new(); let mut bytes=0;
        for _ in 0..20 {
            enqueue_pending(&mut queue,&mut bytes,PendingDelivery{source_id:"a".into(),source_key:String::new(),source_ip:large.src(),packet:large.clone(),reply_only:false,deadline:now+PENDING_TTL,target_id:"b".into(),target_key:String::new(),target_ip:Ipv4Addr::UNSPECIFIED,forward_id:None,forward_protocol:None,forward_target_port:None});
        }
        assert_eq!(queue.len(),PENDING_BYTES/large.bytes().len());
        assert!(bytes<=PENDING_BYTES);

        let delivery=PendingDelivery{source_id:"a".into(),source_key:String::new(),source_ip:packet.src(),packet,reply_only:false,deadline:now+PENDING_TTL,target_id:"b".into(),target_key:String::new(),target_ip:Ipv4Addr::UNSPECIFIED,forward_id:None,forward_protocol:None,forward_target_port:None};
        assert!(!delivery.expired_at(now+PENDING_TTL-Duration::from_millis(1)));
        assert!(delivery.expired_at(now+PENDING_TTL));
    }

    #[test]
    fn persisted_peer_address_validation_accepts_last_peer_ip_and_rejects_reserved_addresses() {
        for (ip, expected) in [("10.77.0.254", true), ("10.77.0.1", false), ("10.77.0.255", false)] {
            let store=Store::open(":memory:").unwrap();
            store.bind_test_identity();store.setup("10.77.0.0/24","hub.example:51820",25).unwrap();
            store.add_group(&Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]}).unwrap();
            store.add_peer(&Peer{id:"p".into(),name:"p".into(),public_key:"key".into(),ipv4:ip.into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
            let mut snapshot=store.runtime_snapshot().unwrap();
            snapshot.forwards.clear();
            assert_eq!(validate_persisted_addresses(&snapshot).is_ok(),expected,"{ip}");
        }
    }

    #[test]
    fn snapshot_retry_schedule_caps_and_resets_after_success() {
        let start = Instant::now();
        let mut retry = SnapshotRetry::new();
        let mut at = start;
        for (next_delay, elapsed) in [(2,1), (4,3), (8,7), (16,15), (30,31), (30,61), (30,91)] {
            at = retry.failed_at(at);
            assert_eq!(at.duration_since(start), Duration::from_secs(elapsed));
            assert_eq!(retry.delay, Duration::from_secs(next_delay));
        }
        retry.succeeded();
        assert_eq!(retry.failed_at(start).duration_since(start), RETRY_INITIAL);
    }

    #[test]
    fn udp_receive_error_classifier_recovers_expected_datagram_errors_only() {
        for kind in [std::io::ErrorKind::Interrupted, std::io::ErrorKind::ConnectionReset, std::io::ErrorKind::ConnectionRefused, std::io::ErrorKind::AddrNotAvailable] {
            assert!(recoverable_udp_error(kind), "{kind:?}");
        }
        for kind in [std::io::ErrorKind::PermissionDenied, std::io::ErrorKind::NotConnected, std::io::ErrorKind::Other] {
            assert!(!recoverable_udp_error(kind), "{kind:?}");
        }
    }

    #[tokio::test]
    async fn udp_receive_task_survives_recoverable_errors_and_fails_on_fatal_error() {
        async fn injected_receiver(mut incoming: mpsc::UnboundedReceiver<std::io::Result<u8>>, readiness: Readiness) -> Result<u8, ()> {
            loop {
                match classify_udp_receive(incoming.recv().await.expect("injected receive result"), &readiness)? {
                    Some(datagram) => return Ok(datagram),
                    None => continue,
                }
            }
        }
        let readiness=Readiness::default();readiness.set(true);
        let (tx,rx)=mpsc::unbounded_channel();
        let task=tokio::spawn(injected_receiver(rx,readiness.clone()));
        for kind in [std::io::ErrorKind::Interrupted,std::io::ErrorKind::ConnectionReset,std::io::ErrorKind::ConnectionRefused] {
            tx.send(Err(std::io::Error::from(kind))).unwrap();
        }
        tx.send(Ok(42)).unwrap();
        assert_eq!(timeout(Duration::from_secs(1),task).await.unwrap().unwrap(),Ok(42));
        assert!(readiness.is_ready(),"recoverable receive errors do not degrade readiness");

        let fatal_readiness=Readiness::default();fatal_readiness.set(true);
        let (tx,rx)=mpsc::unbounded_channel();
        let task=tokio::spawn(injected_receiver(rx,fatal_readiness.clone()));
        tx.send(Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))).unwrap();
        assert_eq!(timeout(Duration::from_secs(1),task).await.unwrap().unwrap(),Err(()));
        assert!(!fatal_readiness.is_ready(),"fatal receive error clears readiness before task exit");
    }

    #[test]
    fn invalid_late_peer_does_not_partially_install_snapshot() {
        let group = Group { id: "g".into(), name: "g".into(), allowed_groups: vec![] };
        let secret = StaticSecret::from([71u8; 32]);
        let key = base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret).as_bytes());
        let peer = |id: &str, public_key: &str| Peer { id: id.into(), name: id.into(), public_key: public_key.into(), ipv4: format!("10.77.0.{}", if id == "a" { 2 } else { 3 }), group_id: "g".into(), received_bytes: 0, sent_bytes: 0, last_handshake_unix: None };
        let mut installed = HashMap::new();
        let mut prior = peer("prior", &key);
        prior.id = "old".into();
        let index = 0x100;
        installed.insert("old".into(), RuntimePeer { peer: prior, group: Some(group.clone()), tunnel: Tunn::new(StaticSecret::from([72u8;32]), PublicKey::from(&secret), None, None, index >> 8, None), endpoint: None, last_data_unix: None, receiver_index: index });
        let indexes = HashMap::from([(index, "old".to_owned())]);
        let before_indexes = indexes.clone();
        let mut state = RouterState { peers: installed, indexes, next_index: 0, ..RouterState::default() };
        let before_peer_ids: Vec<_> = state.peers.keys().cloned().collect();
        let mut invalid_late = vec![peer("a", &key), peer("z", "not-a-valid-public-key")];
        // Valid peer comes first to prove that its provisional install is not published.
        invalid_late[0].id = "a".into();
        assert!(install_peers(&[group.clone()], invalid_late, [73u8;32], Arc::new(RateLimiter::new(&PublicKey::from(&secret),100)), &mut state, HashMap::new()).is_err());
        assert_eq!(state.peers.keys().cloned().collect::<Vec<_>>(), before_peer_ids);
        assert_eq!(state.indexes, before_indexes);
    }

    #[test]
    fn reload_reconcile_removes_new_udp_service_port_collision_only() {
        fn forward(id:&str,protocol:&str,target_port:u16)->Forward {
            Forward{id:id.into(),name:id.into(),protocol:protocol.into(),target_peer_id:"backend".into(),target_port,allowed_group_ids:vec!["source".into()]}
        }
        fn runtime_peer(id:&str,ip:&str,group:Group,secret:[u8;32],receiver_index:u32)->RuntimePeer {
            let public=PublicKey::from(&StaticSecret::from(secret));
            let peer=Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(public.as_bytes()),ipv4:ip.into(),group_id:group.id.clone(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
            RuntimePeer{peer,group:Some(group),tunnel:Tunn::new(StaticSecret::from(secret),PublicKey::from(&StaticSecret::from([99;32])),None,None,receiver_index>>8,None),endpoint:None,last_data_unix:None,receiver_index}
        }
        fn install_flow(flows:&mut Flows,forward:&Forward,source:&Peer,backend:&Peer,source_port:u16,protocol:u8,flags:u8)->u16 {
            let packet=ipv4::validate_forwarded(&service_packet(protocol,source.ipv4.parse::<Ipv4Addr>().unwrap().octets(),[10,77,0,1],source_port,forward.target_port,flags,b"req")).unwrap();
            let (translated,reservation)=flows.prepare_forward_packet(&packet,source,forward,backend,Instant::now()).unwrap();
            let snat=u16::from_be_bytes(translated[20..22].try_into().unwrap());
            flows.complete(reservation,true,Instant::now());
            snat
        }
        let source_group=Group{id:"source".into(),name:"source".into(),allowed_groups:vec!["backend".into()]};
        let backend_group=Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]};
        let mut peers: HashMap<String, RuntimePeer> = HashMap::new();
        peers.insert("source".into(),runtime_peer("source","10.77.0.2",source_group.clone(),[91;32],0x100));
        peers.insert("backend".into(),runtime_peer("backend","10.77.0.3",backend_group,[92;32],0x200));
        let source=peers["source"].peer.clone();let backend=peers["backend"].peer.clone();
        let udp=forward("udp-9000","udp",9000);let tcp=forward("tcp-9000","tcp",9000);
        let old_forwards=vec![udp.clone(),tcp.clone()];
        let mut flows=Flows::new("10.77.0.1".parse().unwrap(),&old_forwards);
        let udp_collision=install_flow(&mut flows,&udp,&source,&backend,1234,17,0);
        let udp_preserved=install_flow(&mut flows,&udp,&source,&backend,1235,17,0);
        let tcp_same_number=install_flow(&mut flows,&tcp,&source,&backend,1236,6,2);
        assert_eq!((udp_collision,udp_preserved,tcp_same_number),(40000,40001,40000));

        // A newly configured UDP service claims the old UDP mapping's SNAT port.
        let mut new_forwards=old_forwards.clone();new_forwards.push(forward("udp-40000","udp",40000));
        flows.reconcile(Some("10.77.0.1".parse().unwrap()),Some("10.77.0.1".parse().unwrap()),&old_forwards,&new_forwards,|id| peers.get(id).map(RuntimePeer::policy));

        let reply=|protocol,source_port,destination_port|ipv4::validate_forwarded(&service_packet(protocol,[10,77,0,3],[10,77,0,1],source_port,destination_port,0,b"reply")).unwrap();
        assert!(flows.lookup_reply(&reply(17,9000,40000),&backend,Instant::now()).is_none(),"stale UDP reply must not shadow the newly configured UDP service");
        let (target,_,reservation)=flows.lookup_reply(&reply(17,9000,40001),&backend,Instant::now()).expect("non-colliding UDP mapping remains active");
        assert_eq!(target,"source");flows.complete(reservation,true,Instant::now());
        let (target,_,reservation)=flows.lookup_reply(&reply(6,9000,40000),&backend,Instant::now()).expect("same numeric TCP mapping is protocol-independent");
        assert_eq!(target,"source");flows.complete(reservation,true,Instant::now());
        assert_eq!(flows.test_state_counts().0,2);
    }

    #[test]
    fn forward_requires_both_allowlist_and_directed_backend_acl() {
        let forward=Forward{id:"f".into(),name:"f".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:8080,allowed_group_ids:vec!["a".into()]};
        let group=Group{id:"a".into(),name:"a".into(),allowed_groups:vec!["b".into()]};
        assert!(policy::forward_allowed(&forward,"a",Some(&group),"b"));
        assert!(!policy::forward_allowed(&forward,"c",Some(&group),"b"));
        let denied=Group{allowed_groups:vec![],..group};
        assert!(!policy::forward_allowed(&forward,"a",Some(&denied),"b"));
    }
    #[test]
    fn receiver_indexes_are_unique() {
        let mut next = 0;
        let mut map = HashMap::new();
        let first = allocate_index(&mut next, &map).unwrap();
        map.insert(first, "a".into());
        let second = allocate_index(&mut next, &map).unwrap();
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn forward_load_errors_fail_closed_and_are_acknowledged() {
        async fn broken_store(path: &std::path::Path) -> Arc<Store> {
            let store=Arc::new(Store::open(path.to_str().unwrap()).unwrap());
            store.bind_test_identity();store.setup("10.77.0.0/24","hub.example:51820",25).unwrap();
            store.add_group(&Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]}).unwrap();
            let secret=StaticSecret::from([41u8;32]);
            store.add_peer(&Peer{id:"p".into(),name:"p".into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret).as_bytes()),ipv4:"10.77.0.2".into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
            create_forward_for_test(&store,Forward{id:"f".into(),name:"f".into(),protocol:"udp".into(),target_peer_id:"p".into(),target_port:53,allowed_group_ids:vec!["g".into()]});
            store
        }
        let dir=tempfile::tempdir().unwrap();
        let startup_store=broken_store(&dir.path().join("startup.sqlite")).await;
        rusqlite::Connection::open(dir.path().join("startup.sqlite")).unwrap().execute("UPDATE forwards SET allowed='not-json'",[]).unwrap();
        let socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (_tx,rx)=mpsc::channel(1);let (ready,wait)=oneshot::channel();
        let readiness=Readiness::default();let stats=RuntimeStats::default();
        let task=tokio::spawn(run_udp(socket,startup_store,[42;32],rx,stats.clone(),readiness.clone(),Some(ready)));
        assert!(wait.await.unwrap().is_err(),"failed initial snapshot is reported to startup");
        assert!(!readiness.is_ready(),"invalid startup snapshot is not ready");
        assert!(task.await.unwrap().is_err(),"failed startup does not enter the runtime loop");

        let reload_store=broken_store(&dir.path().join("reload.sqlite")).await;
        let socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (tx,rx)=mpsc::channel(1);let (ready,wait)=oneshot::channel();
        let readiness=Readiness::default();let stats=RuntimeStats::default();
        let task=tokio::spawn(run_udp(socket,reload_store.clone(),[43;32],rx,stats.clone(),readiness.clone(),Some(ready)));
        wait.await.unwrap().unwrap();
        assert!(readiness.is_ready());
        rusqlite::Connection::open(dir.path().join("reload.sqlite")).unwrap().execute("UPDATE forwards SET allowed='not-json'",[]).unwrap();
        let (ack,ack_wait)=oneshot::channel();tx.send(ReloadCommand{ack}).await.unwrap();
        assert!(ack_wait.await.unwrap().is_err(),"reload reports invalid persisted forward state");
        assert!(!readiness.is_ready(),"failed reload marks runtime degraded");
        assert!(stats.read().await.is_empty(),"failed reload clears runtime peer stats");
        time::sleep(Duration::from_millis(1200)).await;
        assert!(!readiness.is_ready(),"failed retry remains fail-closed");
        rusqlite::Connection::open(dir.path().join("reload.sqlite")).unwrap().execute("UPDATE forwards SET allowed='[\"g\"]'",[]).unwrap();
        let deadline=Instant::now()+Duration::from_secs(4);
        while !readiness.is_ready()&&Instant::now()<deadline {time::sleep(Duration::from_millis(50)).await;}
        assert!(readiness.is_ready(),"timer recovers after a complete valid snapshot is restored");
        assert!(stats.read().await.contains_key("p"),"recovery installs the latest persisted peer snapshot");
        task.abort();
    }

    #[tokio::test]
    async fn malformed_snapshot_recovery_installs_only_latest_policy_and_peers() {
        timeout(Duration::from_secs(20), async {
            let dir=tempfile::tempdir().unwrap();
            let db=dir.path().join("latest-policy.sqlite");
            let store=Arc::new(Store::open(db.to_str().unwrap()).unwrap());
            store.bind_test_identity();store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();
            for group in [
                Group{id:"a".into(),name:"A".into(),allowed_groups:vec!["b".into()]},
                Group{id:"c".into(),name:"C".into(),allowed_groups:vec!["b".into()]},
                Group{id:"b".into(),name:"B".into(),allowed_groups:vec![]},
            ] { store.add_group(&group).unwrap(); }
            store.set_acl("a", &["b".into()]).unwrap();store.set_acl("c", &["b".into()]).unwrap();
            let secrets=[StaticSecret::from([81u8;32]),StaticSecret::from([82u8;32]),StaticSecret::from([83u8;32])];
            for ((id,ip,group),secret) in [("a","10.88.0.2","a"),("b","10.88.0.3","b"),("c","10.88.0.4","c")].into_iter().zip(secrets.iter()) {
                store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
            }
            let hub_private=[84u8;32];let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
            let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();let address=server.local_addr().unwrap();
            let (commands,receiver)=mpsc::channel(2);let (ready,started)=oneshot::channel();
            let readiness=Readiness::default();let stats=RuntimeStats::default();
            let task=tokio::spawn(run_udp(server,store.clone(),hub_private,receiver,stats.clone(),readiness.clone(),Some(ready)));
            started.await.unwrap().unwrap();assert!(readiness.is_ready());
            let sockets=[UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap()];
            let mut clients:Vec<Tunn>=secrets.iter().cloned().enumerate().map(|(i,s)|Tunn::new(s,hub_public,None,None,90+i as u32,None)).collect();
            let mut tx=vec![0;65535];let mut rx=vec![0;65535];
            for i in 0..3 { establish_client(&sockets[i],address,&mut clients[i],&mut tx,&mut rx).await; }
            let packet=|src,dst,payload|service_packet(17,src,dst,12000,9000,0,payload);
            for (index,source,dest,payload) in [(0,[10,88,0,2],[10,88,0,3],b"initial-a".as_slice()),(2,[10,88,0,4],[10,88,0,3],b"initial-c".as_slice())] {
                send_inner(&sockets[index],address,&mut clients[index],&packet(source,dest,payload),&mut tx).await;
                assert_eq!(&timeout(Duration::from_secs(2),recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx)).await.unwrap()[28..],payload);
            }

            rusqlite::Connection::open(&db).unwrap().execute("UPDATE groups SET allowed='broken-json' WHERE id='c'",[]).unwrap();
            let (ack,wait)=oneshot::channel();commands.send(ReloadCommand{ack}).await.unwrap();
            assert!(wait.await.unwrap().is_err());assert!(!readiness.is_ready());
            assert!(stats.read().await.is_empty(),"failed snapshot publishes no partial peer statistics");
            // Mutate the still-malformed database while fail-closed: revoke A,
            // and add a new group/peer. Timer retries must not expose a partial install.
            store.set_acl("a", &[]).unwrap();
            store.add_group(&Group{id:"new".into(),name:"New".into(),allowed_groups:vec![]}).unwrap();
            let d_secret=StaticSecret::from([85u8;32]);
            store.add_peer(&Peer{id:"d".into(),name:"D".into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&d_secret).as_bytes()),ipv4:"10.88.0.5".into(),group_id:"new".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
            time::sleep(Duration::from_millis(2200)).await;
            assert!(!readiness.is_ready());assert!(stats.read().await.is_empty(),"retry failure remains empty and fail-closed");

            rusqlite::Connection::open(&db).unwrap().execute("UPDATE groups SET allowed='[\"b\"]' WHERE id='c'",[]).unwrap();
            let deadline=Instant::now()+Duration::from_secs(4);
            while !readiness.is_ready()&&Instant::now()<deadline {time::sleep(Duration::from_millis(40)).await;}
            assert!(readiness.is_ready(),"timer retries the repaired latest snapshot without another reload");
            assert!(stats.read().await.contains_key("d"),"latest complete snapshot includes the added peer");
            clients[0]=Tunn::new(secrets[0].clone(),hub_public,None,None,90,None);
            clients[1]=Tunn::new(secrets[1].clone(),hub_public,None,None,91,None);
            clients[2]=Tunn::new(secrets[2].clone(),hub_public,None,None,92,None);
            for i in [0,1,2] { establish_client(&sockets[i],address,&mut clients[i],&mut tx,&mut rx).await; }
            send_inner(&sockets[0],address,&mut clients[0],&packet([10,88,0,2],[10,88,0,3],b"revoked-a"),&mut tx).await;
            assert_no_inner(&sockets[1],address,&mut clients[1],&mut rx,&mut tx).await;
            send_inner(&sockets[2],address,&mut clients[2],&packet([10,88,0,4],[10,88,0,3],b"allowed-c"),&mut tx).await;
            assert_eq!(&timeout(Duration::from_secs(2),recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx)).await.unwrap()[28..],b"allowed-c");
            task.abort();
        }).await.expect("bounded malformed snapshot recovery test");
    }

    async fn establish_client(socket: &UdpSocket, address: SocketAddr, client: &mut Tunn, tx: &mut [u8], rx: &mut [u8]) {
        let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],tx) else { panic!("expected handshake initiation") };
        socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),socket.recv_from(rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2,"hub responds to authenticated initiation");
        if let TunnResult::WriteToNetwork(reply)=client.decapsulate(None,&rx[..n],tx) { socket.send_to(reply,address).await.unwrap(); }
    }

    fn transport_checksum(src: [u8; 4], dst: [u8; 4], proto: u8, segment: &[u8]) -> u16 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&src); bytes.extend_from_slice(&dst);
        bytes.extend_from_slice(&[0, proto]); bytes.extend_from_slice(&(segment.len() as u16).to_be_bytes());
        bytes.extend_from_slice(segment);
        let mut sum = 0u32;
        for c in bytes.chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        if bytes.len() % 2 != 0 { sum += (bytes[bytes.len()-1] as u32) << 8; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        !(sum as u16)
    }

    fn service_packet(proto: u8, src: [u8; 4], dst: [u8; 4], sport: u16, dport: u16, flags: u8, payload: &[u8]) -> Vec<u8> {
        let transport_len = (if proto == 6 { 20 } else { 8 }) + payload.len();
        let mut p = vec![0; 20 + transport_len];
        let packet_len = p.len() as u16;
        p[0] = 0x45; p[2..4].copy_from_slice(&packet_len.to_be_bytes()); p[8] = 64; p[9] = proto;
        p[12..16].copy_from_slice(&src); p[16..20].copy_from_slice(&dst);
        p[20..22].copy_from_slice(&sport.to_be_bytes()); p[22..24].copy_from_slice(&dport.to_be_bytes());
        if proto == 6 { p[32] = 0x50; p[33] = flags; p[40..].copy_from_slice(payload); }
        else { p[24..26].copy_from_slice(&(transport_len as u16).to_be_bytes()); p[28..].copy_from_slice(payload); }
        let c = transport_checksum(src, dst, proto, &p[20..]);
        if proto == 6 { p[36..38].copy_from_slice(&c.to_be_bytes()); } else { p[26..28].copy_from_slice(&c.to_be_bytes()); }
        let mut sum = 0u32; for c in p[..20].chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        p[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes()); p
    }

    fn assert_packet_checksums(p: &[u8]) {
        let mut sum = 0u32; for c in p[..20].chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        assert_eq!(!(sum as u16), 0, "IPv4 checksum");
        let proto = p[9]; let checksum = if proto == 6 { u16::from_be_bytes([p[36], p[37]]) } else { u16::from_be_bytes([p[26], p[27]]) };
        if proto == 6 || checksum != 0 { assert_eq!(transport_checksum(p[12..16].try_into().unwrap(), p[16..20].try_into().unwrap(), proto, &p[20..]), 0, "TCP/UDP checksum"); }
    }

    #[test]
    fn shared_rate_limiter_cookie_challenge_accepts_same_source_retry() {
        let hub_secret=StaticSecret::from([201u8;32]);
        let hub_public=PublicKey::from(&hub_secret);
        let limiter=Arc::new(RateLimiter::new(&hub_public,0));
        let mut hub=Tunn::new(hub_secret,PublicKey::from(&StaticSecret::from([202u8;32])),None,None,1,Some(limiter.clone()));
        let mut client=Tunn::new(StaticSecret::from([202u8;32]),hub_public,None,None,2,None);
        let mut tx=vec![0;65535];let mut out=vec![0;65535];
        let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],&mut tx) else {panic!("expected initiation")};
        let addr="127.0.0.1:12345".parse::<SocketAddr>().unwrap();
        let TunnResult::WriteToNetwork(cookie)=limiter.verify_packet(Some(addr.ip()),init,&mut out).unwrap_err() else {panic!("under-load initiation receives cookie challenge")};
        assert!(matches!(client.decapsulate(Some(addr.ip()),cookie,&mut tx),TunnResult::Done));
        let TunnResult::WriteToNetwork(retry)=client.format_handshake_initiation(&mut tx,true) else {panic!("cookie retry initiation")};
        let packet=limiter.verify_packet(Some(addr.ip()),retry,&mut out).expect("same limiter accepts valid MAC2 retry");
        assert!(matches!(packet,Packet::HandshakeInit(_)));
        assert!(matches!(hub.decapsulate(Some(addr.ip()),retry,&mut out),TunnResult::WriteToNetwork(_)));
    }

    #[tokio::test]
    async fn zero_peer_router_resets_shared_cookie_limiter_on_timer_and_reload() {
        let store=Arc::new(Store::open(":memory:").unwrap());
        store.bind_test_identity();
        let hub_private=[211u8;32];
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address=server.local_addr().unwrap();
        let (commands,receiver)=mpsc::channel(1);let (ready,started)=oneshot::channel();
        let readiness=Readiness::default();
        tokio::spawn(run_udp(server,store,hub_private,receiver,RuntimeStats::default(),readiness.clone(),Some(ready)));
        started.await.unwrap().unwrap();
        assert!(readiness.is_ready(),"successful UDP startup with no setup is ready");
        let sender=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let mut tx=vec![0;65535];let mut rx=vec![0;65535];

        // Saturate the router-owned limiter with valid MAC1 initiations while
        // there are no configured peers; the threshold challenge proves these
        // packets reached the shared cookie domain before any peer lookup.
        let mut challenged_client=Tunn::new(StaticSecret::from([212u8;32]),hub_public,None,None,1,None);
        for i in 0..110u8 {
            let mut client=Tunn::new(StaticSecret::from([i.wrapping_add(1);32]),hub_public,None,None,10+i as u32,None);
            if let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],&mut tx) { sender.send_to(init,address).await.unwrap(); }
        }
        while timeout(Duration::from_millis(30),sender.recv_from(&mut rx)).await.is_ok() {}
        let TunnResult::WriteToNetwork(first)=challenged_client.encapsulate(&[],&mut tx) else {panic!("expected initiation")};
        sender.send_to(first,address).await.unwrap();
        let (n,_)=timeout(Duration::from_secs(1),sender.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),3,"shared limiter emits a cookie under load");
        assert!(matches!(challenged_client.decapsulate(Some(address.ip()),&rx[..n],&mut tx),TunnResult::Done));
        // Drain load challenges before waiting for the one-second reset tick.
        while timeout(Duration::from_millis(30),sender.recv_from(&mut rx)).await.is_ok() {}
        time::sleep(Duration::from_millis(1100)).await;

        // The same router-owned limiter must have reset despite having zero
        // peers. An unknown peer is silently ignored after verification; a
        // stale limiter would instead answer with another cookie challenge.
        let mut after_tick=Tunn::new(StaticSecret::from([213u8;32]),hub_public,None,None,222,None);
        let TunnResult::WriteToNetwork(init)=after_tick.encapsulate(&[],&mut tx) else {panic!("expected post-tick initiation")};
        sender.send_to(init,address).await.unwrap();
        assert!(timeout(Duration::from_millis(150),sender.recv_from(&mut rx)).await.is_err(),"zero-peer timer tick resets the shared limiter");

        // Reload does not replace the cookie domain: a retry carrying the
        // router-issued cookie remains verified after acknowledged reload.
        let (ack,wait)=oneshot::channel();commands.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();
        // Re-saturate after the timer reset so this retry must authenticate its
        // MAC2 against the original cookie domain rather than pass under load.
        for i in 0..110u8 {
            let mut client=Tunn::new(StaticSecret::from([i.wrapping_add(31);32]),hub_public,None,None,300+i as u32,None);
            if let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],&mut tx) { sender.send_to(init,address).await.unwrap(); }
        }
        while timeout(Duration::from_millis(30),sender.recv_from(&mut rx)).await.is_ok() {}
        let TunnResult::WriteToNetwork(retry)=challenged_client.format_handshake_initiation(&mut tx,true) else {panic!("cookie retry initiation")};
        sender.send_to(retry,address).await.unwrap();
        assert!(timeout(Duration::from_millis(150),sender.recv_from(&mut rx)).await.is_err(),"reload retained the cookie limiter and accepted its same-source retry");
    }

    async fn send_inner(socket: &UdpSocket, address: SocketAddr, client: &mut Tunn, packet: &[u8], tx: &mut [u8]) {
        let TunnResult::WriteToNetwork(wire) = client.encapsulate(packet, tx) else { panic!("expected encrypted packet") };
        socket.send_to(wire, address).await.unwrap();
    }

    async fn recv_inner(socket: &UdpSocket, client: &mut Tunn, rx: &mut [u8], tx: &mut [u8]) -> Vec<u8> {
        loop {
            let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(rx)).await.unwrap().unwrap();
            match client.decapsulate(None, &rx[..n], tx) {
                TunnResult::WriteToTunnelV4(p, _) => return p.to_vec(),
                TunnResult::WriteToNetwork(p) => { /* handshake response; caller needs a socket to send it */ let _ = p; panic!("unexpected hub handshake while awaiting packet"); }
                _ => {}
            }
        }
    }

    async fn assert_no_inner(socket: &UdpSocket, address: SocketAddr, client: &mut Tunn, rx: &mut [u8], tx: &mut [u8]) {
        let until = tokio::time::Instant::now() + Duration::from_millis(250);
        loop {
            let remaining = until.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { return; }
            let Ok(Ok((n,_))) = timeout(remaining, socket.recv_from(rx)).await else { return };
            match client.decapsulate(None,&rx[..n],tx) {
                TunnResult::WriteToTunnelV4(_,_) => panic!("denied flow delivered application IPv4"),
                TunnResult::WriteToNetwork(packet) => { let _=socket.send_to(packet,address).await; },
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn boringtun_nat_tcp_udp_roundtrip_and_live_policy_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("nat-e2e.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("192.168.44.0/24","hub.example:51820",25).unwrap();
        for g in [
            Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]},
            Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]},
            Group{id:"denied".into(),name:"denied".into(),allowed_groups:vec![]},
        ] { store.add_group(&g).unwrap(); }
        store.set_acl("clients", &["backend".into()]).unwrap();
        // Isolate the forward allowlist assertion: C's group ACL itself permits
        // access to the backend, but its group is not in the forward allowlist.
        store.set_acl("denied", &["backend".into()]).unwrap();
        let secrets = [StaticSecret::from([31u8;32]), StaticSecret::from([32u8;32]), StaticSecret::from([33u8;32]), StaticSecret::from([34u8;32])];
        let specs = [("a","192.168.44.2","clients"),("b","192.168.44.3","backend"),("c","192.168.44.4","denied"),("d","192.168.44.5","backend")];
        for ((id,ip,group), secret) in specs.iter().zip(secrets.iter()) {
            store.add_peer(&Peer{id:(*id).into(),name:(*id).into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:(*ip).into(),group_id:(*group).into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        create_forward_for_test(&store,Forward{id:"f".into(),name:"service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:8080,allowed_group_ids:vec!["clients".into()]});
        create_forward_for_test(&store,Forward{id:"fu".into(),name:"udp service".into(),protocol:"udp".into(),target_peer_id:"b".into(),target_port:8080,allowed_group_ids:vec!["clients".into()]});
        create_forward_for_test(&store,Forward{id:"reserved".into(),name:"reserved tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:40000,allowed_group_ids:vec!["clients".into()]});
        let hub_private=[35u8;32]; let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(4); let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready))); ready_rx.await.unwrap().unwrap();
        let sockets = [UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap()];
        let mut clients: Vec<Tunn> = secrets.into_iter().enumerate().map(|(i,s)|Tunn::new(s,hub_public,None,None,60+i as u32,None)).collect();
        let mut tx=vec![0;65535]; let mut rx=vec![0;65535];
        for i in 0..4 { establish_client(&sockets[i],address,&mut clients[i],&mut tx,&mut rx).await; }

        let mut translated_reply = None;
        let mut total_request_bytes=0u64;
        let mut total_reply_bytes=0u64;
        let mut requests=Vec::new();
        for (_proto,number,flags) in [("tcp",6,0x02),("udp",17,0)] {
            let req=service_packet(number,[192,168,44,2],[192,168,44,1],12345,8080,flags,b"request");
            total_request_bytes+=req.len() as u64;
            let src_peer=store.peers().unwrap().into_iter().find(|p|p.id=="a").unwrap(); let source_group=store.group("clients").unwrap().unwrap();
            assert!(ipv4::validate_and_forward(&req,&src_peer,Some(&source_group)).is_some(),"fixture must pass authenticated IPv4 validation");
            send_inner(&sockets[0],address,&mut clients[0],&req,&mut tx).await;
            requests.push((number,req));
        }
        // Both requests are outstanding before either backend reply arrives.
        let mut mapped_requests=Vec::new();
        for (number,req) in &requests {
            let translated=recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx).await;
            assert_eq!(&translated[12..16],&[192,168,44,1]); assert_eq!(&translated[16..20],&[192,168,44,3]);
            let snat_port=u16::from_be_bytes([translated[20],translated[21]]);
            assert_eq!(snat_port,if *number==6{40001}else{40000},"initial protocol-specific SNAT allocation"); assert_eq!(u16::from_be_bytes([translated[22],translated[23]]),8080);
            assert_eq!(translated[8],63,"SNAT/DNAT must not decrement TTL a second time"); assert_packet_checksums(&translated);
            assert_eq!(translated[9],*number); let payload_at=if *number==6{40}else{28}; assert_eq!(&translated[payload_at..],&req[payload_at..]);
            mapped_requests.push((*number,snat_port));
        }
        for (number,snat_port) in mapped_requests.iter().rev() {
            let reply=service_packet(*number,[192,168,44,3],[192,168,44,1],8080,*snat_port,if *number==6{0x12}else{0},b"reply");
            total_reply_bytes+=reply.len() as u64;
            translated_reply=Some(reply.clone());
            send_inner(&sockets[1],address,&mut clients[1],&reply,&mut tx).await;
        }
        for (number,_) in requests.iter().rev() {
            let restored=recv_inner(&sockets[0],&mut clients[0],&mut rx,&mut tx).await;
            assert_eq!(&restored[12..16],&[192,168,44,1]); assert_eq!(&restored[16..20],&[192,168,44,2]);
            assert_eq!(u16::from_be_bytes([restored[20],restored[21]]),8080); assert_eq!(u16::from_be_bytes([restored[22],restored[23]]),12345);
            assert_eq!(restored[9],*number); let payload_at=if *number==6{40}else{28}; assert_eq!(&restored[payload_at..],b"reply");
            assert_eq!(restored[8],63); assert_packet_checksums(&restored);
        }
        // Warm direct UDP is stateful in both directions and admits only the
        // exact authenticated reverse tuple.
        let direct = service_packet(17,[192,168,44,2],[192,168,44,3],12345,9090,0,b"direct");
        send_inner(&sockets[0],address,&mut clients[0],&direct,&mut tx).await;
        let delivered = recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx).await;
        assert_eq!(&delivered[12..20],&direct[12..20]);
        total_request_bytes += direct.len() as u64;
        let direct_reply = service_packet(17,[192,168,44,3],[192,168,44,2],9090,12345,0,b"direct-reply");
        send_inner(&sockets[1],address,&mut clients[1],&direct_reply,&mut tx).await;
        let restored = recv_inner(&sockets[0],&mut clients[0],&mut rx,&mut tx).await;
        assert_eq!(&restored[12..20],&direct_reply[12..20]);
        total_reply_bytes += direct_reply.len() as u64;
        for invalid in [
            service_packet(17,[192,168,44,3],[192,168,44,2],9091,12345,0,b"changed-source-port"),
            service_packet(17,[192,168,44,3],[192,168,44,2],9090,12346,0,b"changed-destination-port"),
        ] {
            send_inner(&sockets[1],address,&mut clients[1],&invalid,&mut tx).await;
            assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        }
        // Counters are plaintext IPv4 bytes delivered across the hub boundary:
        // ingress accounts on the source peer, egress on the destination peer.
        let snapshot=stats.read().await;
        let (a_rx,a_tx,_,_)=snapshot["a"];
        let (b_rx,b_tx,_,_)=snapshot["b"];
        assert_eq!(a_rx,total_request_bytes,"A ingress includes both delivered forward requests");
        assert_eq!(a_tx,total_reply_bytes,"A egress includes both delivered reverse replies");
        assert_eq!(b_rx,total_reply_bytes,"B ingress includes both delivered reverse replies");
        assert_eq!(b_tx,total_request_bytes,"B egress includes both delivered forward requests");
        drop(snapshot);

        // A backend error is related traffic even without a reverse group ACL.
        let udp_quote=service_packet(17,[192,168,44,1],[192,168,44,3],40000,8080,0,b"request");
        let icmp_error=ipv4::test_icmp_error(Ipv4Addr::new(192,168,44,3),Ipv4Addr::new(192,168,44,1),3,4,&udp_quote[..28]);
        send_inner(&sockets[1],address,&mut clients[1],&icmp_error,&mut tx).await;
        let restored=recv_inner(&sockets[0],&mut clients[0],&mut rx,&mut tx).await;
        assert_eq!(&restored[12..20],&[192,168,44,1,192,168,44,2]);
        assert_eq!(&restored[40..48],&[192,168,44,2,192,168,44,1]);
        assert_eq!(u16::from_be_bytes([restored[48],restored[49]]),12345);
        assert_eq!(u16::from_be_bytes([restored[26],restored[27]]),1280);
        assert_eq!(checksum(&restored[..20]),0);assert_eq!(checksum(&restored[20..]),0);assert_eq!(checksum(&restored[28..48]),0);
        let wrong_peer_error=ipv4::test_icmp_error(Ipv4Addr::new(192,168,44,5),Ipv4Addr::new(192,168,44,1),3,4,&udp_quote[..28]);
        send_inner(&sockets[3],address,&mut clients[3],&wrong_peer_error,&mut tx).await;
        assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;

        // A newly added TCP service is installed only after acknowledged reload;
        // existing authorized mappings remain live and the next TCP candidate is excluded.
        create_forward_for_test(&store,Forward{id:"reserved-next".into(),name:"next reserved tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:40001,allowed_group_ids:vec!["clients".into()]});
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        clients[0]=Tunn::new(StaticSecret::from([31u8;32]),hub_public,None,None,60,None);
        clients[1]=Tunn::new(StaticSecret::from([32u8;32]),hub_public,None,None,61,None);
        establish_client(&sockets[0],address,&mut clients[0],&mut tx,&mut rx).await;
        establish_client(&sockets[1],address,&mut clients[1],&mut tx,&mut rx).await;
        clients[2]=Tunn::new(StaticSecret::from([33u8;32]),hub_public,None,None,62,None);
        clients[3]=Tunn::new(StaticSecret::from([34u8;32]),hub_public,None,None,63,None);
        establish_client(&sockets[2],address,&mut clients[2],&mut tx,&mut rx).await;
        establish_client(&sockets[3],address,&mut clients[3],&mut tx,&mut rx).await;
        // The newly configured TCP service claims the old TCP mapping's SNAT
        // port (40001), so that reply must no longer reach A. The UDP mapping
        // uses a protocol-independent port allocation (40000) and remains valid.
        let stale_tcp_reply=service_packet(6,[192,168,44,3],[192,168,44,1],8080,40001,0x12,b"reply");
        send_inner(&sockets[1],address,&mut clients[1],&stale_tcp_reply,&mut tx).await;
        assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        let retained_udp_reply=service_packet(17,[192,168,44,3],[192,168,44,1],8080,40000,0,b"reply");
        send_inner(&sockets[1],address,&mut clients[1],&retained_udp_reply,&mut tx).await;
        let retained_reply=recv_inner(&sockets[0],&mut clients[0],&mut rx,&mut tx).await;
        assert_eq!(&retained_reply[28..],b"reply","non-colliding UDP mapping remains live");
        for (number,flags) in [(6,0x02),(17,0)] {
            let request=service_packet(number,[192,168,44,2],[192,168,44,1],12346,8080,flags,b"after-reload");
            send_inner(&sockets[0],address,&mut clients[0],&request,&mut tx).await;
            let translated=recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx).await;
            let snat = u16::from_be_bytes([translated[20],translated[21]]);
            if number == 6 { assert!(![40000, 40001].contains(&snat), "reserved TCP service ports are excluded"); }
            else { assert_eq!(snat, 40001, "UDP SNAT cursor is retained across reload and remains protocol-specific"); }
        }

        // C has a directed backend ACL, but the current forward allowlist excludes it.
        let denied=service_packet(17,[192,168,44,4],[192,168,44,1],2345,8080,0,b"denied");
        send_inner(&sockets[2],address,&mut clients[2],&denied,&mut tx).await;
        assert!(timeout(Duration::from_millis(200),sockets[1].recv_from(&mut rx)).await.is_err());
        // D is freshly authenticated after policy reload, but spoofing B's inner
        // source is rejected before reverse NAT.
        let spoof=service_packet(17,[192,168,44,3],[192,168,44,1],8080,40000,0,b"spoof");
        send_inner(&sockets[3],address,&mut clients[3],&spoof,&mut tx).await;
        assert!(timeout(Duration::from_millis(200),sockets[0].recv_from(&mut rx)).await.is_err());

        // ACL revoke is synchronously acknowledged and flushes NAT state.
        store.set_acl("clients", &[]).unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        clients[0]=Tunn::new(StaticSecret::from([31u8;32]),hub_public,None,None,60,None);
        clients[1]=Tunn::new(StaticSecret::from([32u8;32]),hub_public,None,None,61,None);
        establish_client(&sockets[0],address,&mut clients[0],&mut tx,&mut rx).await;
        establish_client(&sockets[1],address,&mut clients[1],&mut tx,&mut rx).await;
        // The reload must clear established NAT mappings: a reply translated for
        // the previous flow cannot be delivered to A using its stale mapping.
        send_inner(&sockets[1],address,&mut clients[1],translated_reply.as_ref().unwrap(),&mut tx).await;
        assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        send_inner(&sockets[1],address,&mut clients[1],&icmp_error,&mut tx).await;
        assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        send_inner(&sockets[0],address,&mut clients[0],&service_packet(17,[192,168,44,2],[192,168,44,1],12345,8080,0,b"revoked"),&mut tx).await;
        assert_no_inner(&sockets[1],address,&mut clients[1],&mut rx,&mut tx).await;
        // Restore ACL, then remove the forward and prove its acknowledged removal blocks traffic.
        store.set_acl("clients", &["backend".into()]).unwrap(); store.remove_forward("fu").unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        clients[0]=Tunn::new(StaticSecret::from([31u8;32]),hub_public,None,None,60,None);
        establish_client(&sockets[0],address,&mut clients[0],&mut tx,&mut rx).await;
        send_inner(&sockets[0],address,&mut clients[0],&service_packet(17,[192,168,44,2],[192,168,44,1],12345,8080,0,b"removed"),&mut tx).await;
        assert_no_inner(&sockets[1],address,&mut clients[1],&mut rx,&mut tx).await;
    }

    #[tokio::test]
    async fn only_authenticated_keepalive_can_migrate_peer_endpoint() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("endpoint.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.1.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"a".into(),name:"A".into(),allowed_groups:vec![]}).unwrap();
        store.add_group(&Group{id:"b".into(),name:"B".into(),allowed_groups:vec!["a".into()]}).unwrap();
        store.set_acl("b", &["a".into()]).unwrap();
        let a_secret=StaticSecret::from([17u8;32]);
        let b_secret=StaticSecret::from([18u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.1.0.2","a"),("b",&b_secret,"10.1.0.3","b")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        let hub_private=[19u8;32];
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(1);
        let (ready,ready_rx)=oneshot::channel();
        tokio::spawn(run_udp(server,store,hub_private,commands_rx,RuntimeStats::default(),Readiness::default(),Some(ready)));
        ready_rx.await.unwrap().unwrap();
        let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let original=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let migrated=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let attacker=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret,hub_public,None,None,51,None);
        let mut b=Tunn::new(b_secret,hub_public,None,None,52,None);
        let mut tx=vec![0u8;65535]; let mut rx=vec![0u8;65535];
        establish_client(&original,address,&mut a,&mut tx,&mut rx).await;
        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;

        let TunnResult::WriteToNetwork(keepalive)=a.encapsulate(&[],&mut tx) else {panic!("expected encrypted keepalive")};
        let keepalive=keepalive.to_vec();
        migrated.send_to(&keepalive,address).await.unwrap();
        time::sleep(Duration::from_millis(50)).await;
        let route_to_a=ipv4::test_packet([10,1,0,3],[10,1,0,2],false,false);
        let TunnResult::WriteToNetwork(wire)=b.encapsulate(&route_to_a,&mut tx) else {panic!("expected encrypted routed packet")};
        b_socket.send_to(wire,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),migrated.recv_from(&mut rx)).await.unwrap().unwrap();
        assert!(matches!(a.decapsulate(None,&rx[..n],&mut tx),TunnResult::WriteToTunnelV4(_, _)),"hub uses the new endpoint after authenticated keepalive");
        assert!(timeout(Duration::from_millis(100),original.recv_from(&mut rx)).await.is_err(),"hub stopped using the old endpoint");

        let TunnResult::WriteToNetwork(forged)=a.encapsulate(&[],&mut tx) else {panic!("expected keepalive for forgery")};
        let mut forged=forged.to_vec(); *forged.last_mut().unwrap()^=1;
        attacker.send_to(&forged,address).await.unwrap();
        time::sleep(Duration::from_millis(50)).await;
        attacker.send_to(&keepalive,address).await.unwrap();
        time::sleep(Duration::from_millis(50)).await;
        let TunnResult::WriteToNetwork(wire)=b.encapsulate(&route_to_a,&mut tx) else {panic!("expected encrypted routed packet")};
        b_socket.send_to(wire,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),migrated.recv_from(&mut rx)).await.unwrap().unwrap();
        assert!(matches!(a.decapsulate(None,&rx[..n],&mut tx),TunnResult::WriteToTunnelV4(_, _)),"invalid and replayed packets cannot hijack the endpoint");
        assert!(timeout(Duration::from_millis(100),attacker.recv_from(&mut rx)).await.is_err(),"forged/replayed sender receives no routed packet");
        drop(commands_tx);
    }

    #[tokio::test]
    async fn router_publishes_stats_only_for_authenticated_udp_packets() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("endpoint.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.1.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"a".into(),name:"A".into(),allowed_groups:vec![]}).unwrap();
        let a_secret=StaticSecret::from([17u8;32]);
        store.add_peer(&Peer{id:"a".into(),name:"a".into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&a_secret).as_bytes()),ipv4:"10.1.0.2".into(),group_id:"a".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        let hub_private=[19u8;32];
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(1);
        let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        let (publish_tx,mut publish_rx)=mpsc::unbounded_channel();
        let (packet_tx,mut packet_rx)=mpsc::unbounded_channel();
        let router=run_udp(server,store,hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready));
        tokio::spawn(STATS_PUBLISH_COUNT.scope(std::cell::Cell::new(0),DISABLE_TIMERS_FOR_TEST.scope(true,PACKET_RESULT_OBSERVER.scope(packet_tx,STATS_PUBLISH_OBSERVER.scope(publish_tx,router)))));
        ready_rx.await.unwrap().unwrap();
        // Startup snapshot is expected; drain it before measuring packet-triggered publications.
        let startup_publications=timeout(Duration::from_secs(1),publish_rx.recv()).await.unwrap().unwrap();
        assert_eq!(startup_publications,1);
        let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let original=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let migrated=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let attacker=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret,hub_public,None,None,51,None);
        let mut tx=vec![0u8;65535]; let mut rx=vec![0u8;65535];

        // One installed peer receives index 0x200 (allocate_index starts at 2). This is a
        // syntactically valid Data packet whose receiver index hits, but has no session yet.
        let mut no_session=vec![0u8;32];
        no_session[..4].copy_from_slice(&4u32.to_le_bytes());
        no_session[4..8].copy_from_slice(&0x200u32.to_le_bytes());
        attacker.send_to(&no_session,address).await.unwrap();
        assert!(!timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap(),"index-hit NoCurrentSession Data must be rejected");
        assert!(publish_rx.try_recv().is_err(),"NoCurrentSession Data must not publish stats");

        establish_client(&original,address,&mut a,&mut tx,&mut rx).await;
        assert!(timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap(),"authenticated initiation must be accepted");
        assert_eq!(timeout(Duration::from_secs(1),publish_rx.recv()).await.unwrap().unwrap(),startup_publications+1);
        assert!(timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap(),"authenticated client keepalive completing the handshake must be accepted");
        assert_eq!(timeout(Duration::from_secs(1),publish_rx.recv()).await.unwrap().unwrap(),startup_publications+2);
        assert!(stats.read().await["a"].2.is_some(),"valid handshake updates published handshake stats");

        // Valid keepalive from a different endpoint is authenticated, published and migrates endpoint.
        let TunnResult::WriteToNetwork(keepalive)=a.encapsulate(&[],&mut tx) else {panic!("expected encrypted keepalive")};
        let keepalive=keepalive.to_vec();
        migrated.send_to(&keepalive,address).await.unwrap();
        assert!(timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap());
        assert_eq!(timeout(Duration::from_secs(1),publish_rx.recv()).await.unwrap().unwrap(),startup_publications+3);
        assert!(stats.read().await["a"].3.is_some(),"valid keepalive updates published data stats");

        // Valid encrypted data also traverses the real router packet path and publishes.
        let inner=ipv4::test_packet([10,1,0,2],[10,1,0,99],false,false);
        let TunnResult::WriteToNetwork(data)=a.encapsulate(&inner,&mut tx) else {panic!("expected encrypted data")};
        original.send_to(data,address).await.unwrap();
        assert!(timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap());
        assert_eq!(timeout(Duration::from_secs(1),publish_rx.recv()).await.unwrap().unwrap(),startup_publications+4);

        // Bad tag and exact replay both reach the indexed peer but must not publish.
        let TunnResult::WriteToNetwork(forged)=a.encapsulate(&[],&mut tx) else {panic!("expected keepalive for forgery")};
        let mut forged=forged.to_vec(); *forged.last_mut().unwrap()^=1;
        attacker.send_to(&forged,address).await.unwrap();
        assert!(!timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap(),"bad authentication tag must be rejected");
        assert!(publish_rx.try_recv().is_err(),"bad tag must not publish stats");

        let TunnResult::WriteToNetwork(replay)=a.encapsulate(&[],&mut tx) else {panic!("expected replay packet")};
        let replay=replay.to_vec();
        original.send_to(&replay,address).await.unwrap();
        assert!(timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap());
        assert_eq!(timeout(Duration::from_secs(1),publish_rx.recv()).await.unwrap().unwrap(),startup_publications+5);
        original.send_to(&replay,address).await.unwrap();
        assert!(!timeout(Duration::from_secs(1),packet_rx.recv()).await.unwrap().unwrap(),"exact replay must be rejected");
        assert!(publish_rx.try_recv().is_err(),"exact replay must not publish stats");
        drop(commands_tx);
    }

    #[tokio::test]
    async fn unrelated_reload_preserves_established_client_tunnels_and_direct_udp_flow() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("retain.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.91.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"a".into(),name:"A".into(),allowed_groups:vec!["b".into()]}).unwrap();
        store.add_group(&Group{id:"b".into(),name:"B".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("a", &["b".into()]).unwrap();
        let a_secret=StaticSecret::from([171u8;32]);let b_secret=StaticSecret::from([172u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.91.0.2","a"),("b",&b_secret,"10.91.0.3","b")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        let hub_private=[173u8;32];let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();let address=server.local_addr().unwrap();
        let (commands,receiver)=mpsc::channel(1);let (ready,started)=oneshot::channel();let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,receiver,stats.clone(),Readiness::default(),Some(ready)));started.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret,hub_public,None,None,71,None);let mut b=Tunn::new(b_secret,hub_public,None,None,72,None);
        let mut tx=vec![0;65535];let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;
        let request=service_packet(17,[10,91,0,2],[10,91,0,3],12000,9000,0,b"before-reload");
        send_inner(&a_socket,address,&mut a,&request,&mut tx).await;
        assert_eq!(&recv_inner(&b_socket,&mut b,&mut rx,&mut tx).await[28..],b"before-reload");
        store.add_group(&Group{id:"unrelated".into(),name:"unrelated".into(),allowed_groups:vec![]}).unwrap();
        let (ack,wait)=oneshot::channel();commands.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();

        // Keep the exact same client Tunn objects and use the established UDP tuple.
        let continuation=service_packet(17,[10,91,0,2],[10,91,0,3],12000,9000,0,b"after-reload");
        send_inner(&a_socket,address,&mut a,&continuation,&mut tx).await;
        let delivered=timeout(Duration::from_millis(500),recv_inner(&b_socket,&mut b,&mut rx,&mut tx)).await;
        assert!(delivered.is_ok(),"unrelated reload retains receiver index, tunnel session, and established direct UDP flow");
        assert_eq!(&delivered.unwrap()[28..],b"after-reload");
        let snapshot=stats.read().await;
        assert!(snapshot["a"].0 >= (request.len()+continuation.len()) as u64,"application counters remain cumulative over reload");
    }

    #[tokio::test]
    async fn boringtun_clients_handshake_and_route_only_authorized_ipv4() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("test.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.77.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group { id:"a".into(), name:"A".into(), allowed_groups:vec!["b".into()] }).unwrap();
        store.add_group(&Group { id:"b".into(), name:"B".into(), allowed_groups:vec![] }).unwrap();
        store.set_acl("a", &["b".into()]).unwrap();
        let client_a_secret = StaticSecret::from([7u8;32]);
        let client_b_secret = StaticSecret::from([8u8;32]);
        let client_c_secret = StaticSecret::from([10u8;32]);
        for (id, secret, ip, group) in [("a", &client_a_secret, "10.77.0.2", "a"), ("b", &client_b_secret, "10.77.0.3", "b"), ("c", &client_c_secret, "10.77.0.4", "a")] {
            let public = PublicKey::from(secret);
            let peer = Peer { id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(public.as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None };
            store.add_peer(&peer).unwrap();
        }
        let hub_private=[9u8;32];
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address=server.local_addr().unwrap();
        let (commands_tx, commands_rx)=mpsc::channel(4);
        let stats=RuntimeStats::default();
        let (ready,ready_rx)=oneshot::channel();tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready)));
        ready_rx.await.unwrap().unwrap();
        let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let client_a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_b_migrated_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_c_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut client_a=Tunn::new(client_a_secret.clone(),hub_public,None,None,33,None);
        let mut client_b=Tunn::new(client_b_secret.clone(),hub_public,None,None,34,None);
        let mut client_c=Tunn::new(client_c_secret,hub_public,None,None,35,None);
        let mut tx=vec![0u8;65535]; let mut rx=vec![0u8;65535];
        for (socket, client) in [(&client_a_socket, &mut client_a), (&client_b_socket, &mut client_b)] {
            let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],&mut tx) else { panic!("expected handshake initiation") };
            socket.send_to(init,address).await.unwrap();
            let (n,_) = timeout(Duration::from_secs(3),socket.recv_from(&mut rx)).await.unwrap().unwrap();
            assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2,"server must return a handshake response");
            let result=client.decapsulate(None,&rx[..n],&mut tx);
            match result {
                TunnResult::Done => {},
                TunnResult::WriteToNetwork(packet) => { socket.send_to(packet,address).await.unwrap(); },
                other => panic!("client rejected handshake response: {other:?}"),
            }
        }
        let TunnResult::WriteToNetwork(init)=client_c.encapsulate(&[],&mut tx) else {panic!("expected client C handshake initiation")};
        client_c_socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),client_c_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2,"server must return client C handshake response");
        if let TunnResult::WriteToNetwork(packet)=client_c.decapsulate(None,&rx[..n],&mut tx) { client_c_socket.send_to(packet,address).await.unwrap(); }

        // An unrelated mutation retains the exact established tunnel sessions.
        store.add_group(&Group{id:"unrelated".into(),name:"unrelated".into(),allowed_groups:vec![]}).unwrap();
        let (ack,wait)=oneshot::channel();commands_tx.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();
        // A valid transport packet from a migrated socket still authenticates
        // endpoint migration independently of the reload.
        let TunnResult::WriteToNetwork(keepalive)=client_b.encapsulate(&[],&mut tx) else {panic!("expected B keepalive")};
        client_b_migrated_socket.send_to(keepalive,address).await.unwrap();

        let packet=ipv4::test_packet([10,77,0,2],[10,77,0,3],false,false);
        let TunnResult::WriteToNetwork(wire)=client_a.encapsulate(&packet,&mut tx) else {panic!("expected encrypted data")};
        client_a_socket.send_to(wire,address).await.unwrap();
        let first_inner=recv_inner(&client_b_migrated_socket,&mut client_b,&mut rx,&mut tx).await;
        assert_eq!(&first_inner[12..20],&packet[12..20]);
        assert_eq!(first_inner[8],63,"pending forwarding decrements TTL only once");
        let after_data=stats.read().await;
        assert_eq!(after_data["a"].0,packet.len() as u64);
        assert_eq!(after_data["b"].1,packet.len() as u64);
        drop(after_data);

        // The pending delivery used the authenticated response endpoint; warm
        // traffic continues to route there without another handshake.
        let second_packet=ipv4::test_packet([10,77,0,2],[10,77,0,3],false,false);
        let TunnResult::WriteToNetwork(wire)=client_a.encapsulate(&second_packet,&mut tx) else {panic!("expected second encrypted packet")};
        client_a_socket.send_to(wire,address).await.unwrap();
        let second_inner=loop {
            let (n,_) = timeout(Duration::from_secs(3),client_b_migrated_socket.recv_from(&mut rx)).await.unwrap().unwrap();
            match client_b.decapsulate(None,&rx[..n],&mut tx) {
                TunnResult::WriteToTunnelV4(packet,_) => break packet.to_vec(),
                TunnResult::WriteToNetwork(reply) => { client_b_migrated_socket.send_to(reply,address).await.unwrap(); }
                _ => {}
            }
        };
        assert_eq!(&second_inner[12..20],&second_packet[12..20]);
        assert_eq!(second_inner[8],63,"forwarding decrements TTL");
        assert!(timeout(Duration::from_millis(100),client_b_socket.recv_from(&mut rx)).await.is_err(),"hub continued using the old destination endpoint");

        // Reverse direction is denied, as are same-group routing, source spoofing,
        // malformed headers and both first and non-first IPv4 fragments.
        for (source, destination, _same_group, _spoof, malformed, fragment) in [
            ([10,77,0,3],[10,77,0,2],false,false,false,false),
            ([10,77,0,2],[10,77,0,4],true,false,false,false),
            ([10,77,0,99],[10,77,0,3],false,true,false,false),
            ([10,77,0,2],[10,77,0,3],false,false,true,false),
            ([10,77,0,2],[10,77,0,3],false,false,false,true),
        ] {
            let (socket, client) = if source[3] == 3 { (&client_b_migrated_socket,&mut client_b) } else { (&client_a_socket,&mut client_a) };
            let packet=ipv4::test_packet(source,destination,malformed,fragment);
            let TunnResult::WriteToNetwork(wire)=client.encapsulate(&packet,&mut tx) else {panic!("expected encrypted packet")};
            socket.send_to(wire,address).await.unwrap();
            let (denied_socket,denied_client)=match destination[3] {2=>(&client_a_socket,&mut client_a),3=>(&client_b_migrated_socket,&mut client_b),4=>(&client_c_socket,&mut client_c),_=>unreachable!()};
            assert_no_inner(denied_socket,address,denied_client,&mut rx,&mut tx).await;
        }

        // A successful acknowledgement means the old tunnels and policy are gone.
        store.set_acl("a", &[]).unwrap();
        let (ack,wait)=oneshot::channel();commands_tx.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();
        // The initiating client establishes a fresh session after policy reload.
        client_a=Tunn::new(client_a_secret,hub_public,None,None,33,None);
        let TunnResult::WriteToNetwork(init)=client_a.encapsulate(&[],&mut tx) else {panic!("expected post-reload handshake")};
        client_a_socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),client_a_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2);
        if let TunnResult::WriteToNetwork(packet)=client_a.decapsulate(None,&rx[..n],&mut tx) { client_a_socket.send_to(packet,address).await.unwrap(); }
        let TunnResult::WriteToNetwork(wire)=client_a.encapsulate(&packet,&mut tx) else {panic!("expected encrypted packet")};
        client_a_socket.send_to(wire,address).await.unwrap();
        assert_no_inner(&client_b_migrated_socket,address,&mut client_b,&mut rx,&mut tx).await;

        let unknown=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let unknown_secret=StaticSecret::from([11u8;32]);
        let mut unknown_client=Tunn::new(unknown_secret,hub_public,None,None,35,None);
        let TunnResult::WriteToNetwork(init)=unknown_client.encapsulate(&[],&mut tx) else {panic!("expected initiation")};
        unknown.send_to(init,address).await.unwrap();
        assert!(timeout(Duration::from_millis(150),unknown.recv_from(&mut rx)).await.is_err(),"unknown static key must not receive a response");
    }

    #[tokio::test]
    async fn unrelated_reload_preserves_established_forward_tcp_tunnel_and_nat_tuple() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("cold-forward.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]}).unwrap();
        store.add_group(&Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("clients", &["backend".into()]).unwrap();
        let a_secret=StaticSecret::from([81u8;32]); let b_secret=StaticSecret::from([82u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.88.0.2","clients"),("b",&b_secret,"10.88.0.3","backend")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        create_forward_for_test(&store,Forward{id:"tcp".into(),name:"tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:5353,allowed_group_ids:vec!["clients".into()]});
        let hub_private=[83u8;32]; let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(2); let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready))); ready_rx.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret.clone(),hub_public,None,None,81,None); let mut b=Tunn::new(b_secret.clone(),hub_public,None,None,82,None);
        let mut tx=vec![0;65535]; let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;
        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;

        let request=service_packet(6,[10,88,0,2],[10,88,0,1],12345,5353,0x02,b"syn-data");
        send_inner(&a_socket,address,&mut a,&request,&mut tx).await;
        let delivered=recv_inner(&b_socket,&mut b,&mut rx,&mut tx).await;
        assert_eq!(&delivered[12..16],&[10,88,0,1]); assert_eq!(&delivered[16..20],&[10,88,0,3]);
        let translated_port=u16::from_be_bytes([delivered[20],delivered[21]]);
        assert_ne!(translated_port,12345);
        assert_eq!(u16::from_be_bytes([delivered[22],delivered[23]]),5353);
        assert_eq!(&delivered[40..],b"syn-data");
        assert_eq!(delivered[8],63,"forward packet TTL is decremented exactly once");
        assert_packet_checksums(&delivered);

        // Complete the backend side of the established stream before reload.
        let syn_ack=service_packet(6,[10,88,0,3],[10,88,0,1],5353,translated_port,0x12,b"syn-ack");
        send_inner(&b_socket,address,&mut b,&syn_ack,&mut tx).await;
        let restored=recv_inner(&a_socket,&mut a,&mut rx,&mut tx).await;
        assert_eq!(&restored[12..16],&[10,88,0,1]);
        assert_eq!(&restored[16..20],&[10,88,0,2]);
        assert_eq!(u16::from_be_bytes([restored[20],restored[21]]),5353);
        assert_eq!(u16::from_be_bytes([restored[22],restored[23]]),12345);
        assert_eq!(&restored[40..],b"syn-ack");
        assert_packet_checksums(&restored);

        // Insert an unrelated group, then reload with this connection already
        // established. Keep the exact same client tunnel objects throughout.
        store.add_group(&Group{id:"unrelated".into(),name:"unrelated".into(),allowed_groups:vec![]}).unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();

        // Same client Tunn objects, established TCP tuple, and no fresh SYN or
        // handshake across an unrelated policy reload.
        let ack=service_packet(6,[10,88,0,2],[10,88,0,1],12345,5353,0x10,b"client-data");
        send_inner(&a_socket,address,&mut a,&ack,&mut tx).await;
        let after_reload=recv_inner(&b_socket,&mut b,&mut rx,&mut tx).await;
        assert_eq!(u16::from_be_bytes([after_reload[20],after_reload[21]]),translated_port);
        assert_eq!(&after_reload[40..],b"client-data");
        assert_packet_checksums(&after_reload);
        let backend_data=service_packet(6,[10,88,0,3],[10,88,0,1],5353,translated_port,0x10,b"backend-data");
        send_inner(&b_socket,address,&mut b,&backend_data,&mut tx).await;
        let restored=recv_inner(&a_socket,&mut a,&mut rx,&mut tx).await;
        assert_eq!(&restored[12..16],&[10,88,0,1]);
        assert_eq!(&restored[16..20],&[10,88,0,2]);
        assert_eq!(u16::from_be_bytes([restored[20],restored[21]]),5353);
        assert_eq!(u16::from_be_bytes([restored[22],restored[23]]),12345);
        assert_eq!(&restored[40..],b"backend-data");
        assert_packet_checksums(&restored);
        let snapshot=stats.read().await;
        assert!(snapshot["a"].0 >= (request.len()+ack.len()) as u64);
        assert!(snapshot["b"].1 >= (delivered.len()+after_reload.len()) as u64);
    }

    #[tokio::test]
    async fn cold_forward_two_syns_deliver_after_handshake_with_exact_reverse_mappings() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("cold-two-syns.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]}).unwrap();
        store.add_group(&Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("clients", &["backend".into()]).unwrap();
        let a_secret=StaticSecret::from([91u8;32]); let b_secret=StaticSecret::from([92u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.88.0.2","clients"),("b",&b_secret,"10.88.0.3","backend")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        create_forward_for_test(&store,Forward{id:"tcp".into(),name:"tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:5353,allowed_group_ids:vec!["clients".into()]});
        let hub_private=[93u8;32];let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(2);let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        let (queue_observer,mut observations)=mpsc::unbounded_channel();
        tokio::spawn(QUEUE_OBSERVER.scope(queue_observer,run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready))));ready_rx.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret,hub_public,None,None,91,None);let mut b=Tunn::new(b_secret,hub_public,None,None,92,None);
        let mut tx=vec![0;65535];let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;

        let requests=[
            service_packet(6,[10,88,0,2],[10,88,0,1],12345,5353,0x02,b"first-cold-syn"),
            service_packet(6,[10,88,0,2],[10,88,0,1],12346,5353,0x02,b"second-cold-syn"),
        ];
        for request in &requests { send_inner(&a_socket,address,&mut a,request,&mut tx).await; }
        // Observe the actual application queue after each enqueue, not merely
        // the sender's UDP write or a later reload acknowledgement. This is the
        // barrier proving both SYNs are pending before policy is mutated.
        for expected_count in 1..=2 {
            let observed=timeout(Duration::from_secs(2),observations.recv()).await.unwrap().unwrap();
            assert_eq!(observed.queued.len(),expected_count);
            assert_eq!(observed.queued.iter().map(|(source,target,forward,src,dst,target_ip,port,proto)|(source.as_str(),target.as_str(),forward.as_deref(),*src,*dst,*target_ip,*port,*proto)).collect::<Vec<_>>(),requests[..expected_count].iter().map(|request| ("a","b",Some("tcp"),Ipv4Addr::new(10,88,0,2),Ipv4Addr::new(10,88,0,1),Ipv4Addr::new(10,88,0,3),u16::from_be_bytes([request[20],request[21]]),6)).collect::<Vec<_>>());
            assert_eq!(observed.queued_bytes,requests[..expected_count].iter().map(|request|request.len()).sum::<usize>());
            assert_eq!(observed.flow_counts,(0,0));
            assert_eq!(observed.source_counters,(0,0));
        }
        let before_auth=stats.read().await;
        assert_eq!(before_auth["a"].0,0,"cold forwarded SYNs are not committed before backend authentication");
        assert_eq!(before_auth["b"].1,0,"no plaintext or byte accounting reaches an unauthenticated backend");
        drop(before_auth);

        // Change unrelated persisted policy and apply it while both SYNs are
        // application-owned in the cold-target queue. Keep the tunnels intact.
        store.add_group(&Group{id:"unrelated".into(),name:"unrelated".into(),allowed_groups:vec![]}).unwrap();
        let (ack,wait)=oneshot::channel();commands_tx.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();

        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;
        // An authenticated encrypted keepalive exercises the real UDP path and
        // makes the target's post-handshake readiness explicit.
        let TunnResult::WriteToNetwork(keepalive)=b.encapsulate(&[],&mut tx) else {panic!("expected authenticated keepalive")};
        b_socket.send_to(keepalive,address).await.unwrap();

        let mut mapped=Vec::new();
        for request in &requests {
            // The router may need to initiate its own session to B while
            // draining the pending packets. Complete that real peer-to-peer
            // WireGuard handshake response instead of treating it as app data.
            let delivered=loop {
                let (n,_)=timeout(Duration::from_secs(3),b_socket.recv_from(&mut rx)).await.unwrap().unwrap();
                match b.decapsulate(None,&rx[..n],&mut tx) {
                    TunnResult::WriteToTunnelV4(packet,_)=>break packet.to_vec(),
                    TunnResult::WriteToNetwork(packet)=>{b_socket.send_to(packet,address).await.unwrap();},
                    _=>{}
                }
            };
            assert_eq!(&delivered[12..16],&[10,88,0,1]);assert_eq!(&delivered[16..20],&[10,88,0,3]);
            assert_eq!(u16::from_be_bytes([delivered[22],delivered[23]]),5353);
            assert_eq!(delivered[8],63,"SNAT/DNAT decrements TTL exactly once");
            assert_eq!(&delivered[40..],&request[40..],"each queued SYN payload is delivered unchanged");
            assert_packet_checksums(&delivered);
            mapped.push((u16::from_be_bytes([delivered[20],delivered[21]]),request));
        }
        assert_ne!(mapped[0].0,mapped[1].0,"independent source tuples receive distinct SNAT ports");
        assert_eq!(stats.read().await["a"].0,(requests[0].len()+requests[1].len()) as u64,"source accounting occurs only on actual delivery");

        let mut replies=Vec::new();
        for (snat_port,request) in &mapped {
            let original_port=u16::from_be_bytes([request[20],request[21]]);
            let reply=service_packet(6,[10,88,0,3],[10,88,0,1],5353,*snat_port,0x12,b"syn-ack");
            send_inner(&b_socket,address,&mut b,&reply,&mut tx).await;
            let restored=recv_inner(&a_socket,&mut a,&mut rx,&mut tx).await;
            assert_eq!(&restored[12..16],&[10,88,0,1]);assert_eq!(&restored[16..20],&[10,88,0,2]);
            assert_eq!(u16::from_be_bytes([restored[20],restored[21]]),5353);
            assert_eq!(u16::from_be_bytes([restored[22],restored[23]]),original_port,"reverse mapping restores this SYN's exact original source port");
            assert_eq!(&restored[40..],b"syn-ack");assert_packet_checksums(&restored);
            replies.push(reply);
        }
        assert_no_inner(&b_socket,address,&mut b,&mut rx,&mut tx).await;
        let snapshot=stats.read().await;
        assert_eq!(snapshot["a"].0,(requests[0].len()+requests[1].len()) as u64);
        assert_eq!(snapshot["b"].1,(requests[0].len()+requests[1].len()) as u64);
        assert_eq!(snapshot["a"].1,(replies[0].len()+replies[1].len()) as u64);
        assert_eq!(snapshot["b"].0,(replies[0].len()+replies[1].len()) as u64);
    }

    #[tokio::test]
    async fn cold_forward_deletion_ack_prevents_late_handshake_delivery() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("cold-forward-delete.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]}).unwrap();
        store.add_group(&Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("clients", &["backend".into()]).unwrap();
        let a_secret=StaticSecret::from([101u8;32]); let b_secret=StaticSecret::from([102u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.88.0.2","clients"),("b",&b_secret,"10.88.0.3","backend")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        create_forward_for_test(&store,Forward{id:"tcp".into(),name:"tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:5353,allowed_group_ids:vec!["clients".into()]});
        let hub_private=[103u8;32];let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(2);let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        let (queue_observer,mut observations)=mpsc::unbounded_channel();
        tokio::spawn(QUEUE_OBSERVER.scope(queue_observer,run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready))));ready_rx.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret,hub_public,None,None,101,None);let mut b=Tunn::new(b_secret,hub_public,None,None,102,None);
        let mut tx=vec![0;65535];let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;

        let queued_syn=service_packet(6,[10,88,0,2],[10,88,0,1],12345,5353,0x02,b"must-not-arrive");
        send_inner(&a_socket,address,&mut a,&queued_syn,&mut tx).await;
        let observed=timeout(Duration::from_secs(2),observations.recv()).await.unwrap().unwrap();
        assert_eq!(observed.queued,vec![("a".into(),"b".into(),Some("tcp".into()),Ipv4Addr::new(10,88,0,2),Ipv4Addr::new(10,88,0,1),Ipv4Addr::new(10,88,0,3),12345,6)]);
        assert_eq!(observed.queued_bytes,queued_syn.len());
        assert_eq!(observed.flow_counts,(0,0));
        assert_eq!(observed.source_counters,(0,0));
        assert_eq!(stats.read().await["a"].0,0,"cold forwarded SYN is not committed before backend authentication");

        // Remove the actual persisted forward and wait for the router to publish
        // the new snapshot before B completes its first handshake.
        store.remove_forward("tcp").unwrap();
        let (ack,wait)=oneshot::channel();commands_tx.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();

        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;
        let TunnResult::WriteToNetwork(keepalive)=b.encapsulate(&[],&mut tx) else {panic!("expected authenticated keepalive")};
        b_socket.send_to(keepalive,address).await.unwrap();
        assert_no_inner(&b_socket,address,&mut b,&mut rx,&mut tx).await;
        assert_eq!(stats.read().await["a"].0,0,"acknowledged forward deletion discards the queued SYN without source accounting");

        // The backend remains live and the directed ACL still authorizes direct
        // peer traffic after the forward-only policy change.
        let direct_syn=service_packet(6,[10,88,0,2],[10,88,0,3],23456,5353,0x02,b"direct-syn");
        send_inner(&a_socket,address,&mut a,&direct_syn,&mut tx).await;
        let delivered=recv_inner(&b_socket,&mut b,&mut rx,&mut tx).await;
        assert_eq!(&delivered[12..16],&[10,88,0,2]);assert_eq!(&delivered[16..20],&[10,88,0,3]);
        assert_eq!(u16::from_be_bytes([delivered[20],delivered[21]]),23456);
        assert_eq!(u16::from_be_bytes([delivered[22],delivered[23]]),5353);
        assert_eq!(&delivered[40..],b"direct-syn");assert_packet_checksums(&delivered);

        let direct_reply=service_packet(6,[10,88,0,3],[10,88,0,2],5353,23456,0x12,b"direct-syn-ack");
        send_inner(&b_socket,address,&mut b,&direct_reply,&mut tx).await;
        let restored=recv_inner(&a_socket,&mut a,&mut rx,&mut tx).await;
        assert_eq!(&restored[12..16],&[10,88,0,3]);assert_eq!(&restored[16..20],&[10,88,0,2]);
        assert_eq!(u16::from_be_bytes([restored[20],restored[21]]),5353);
        assert_eq!(u16::from_be_bytes([restored[22],restored[23]]),23456);
        assert_eq!(&restored[40..],b"direct-syn-ack");assert_packet_checksums(&restored);
    }

    #[tokio::test]
    async fn cold_forward_tcp_waits_for_packet_data_without_committing_or_leaking_plaintext() {
        let hub_ip = Ipv4Addr::new(10, 88, 0, 1);
        let source = Peer{id:"a".into(),name:"a".into(),public_key:String::new(),ipv4:"10.88.0.2".into(),group_id:"clients".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let backend = Peer{id:"b".into(),name:"b".into(),public_key:String::new(),ipv4:"10.88.0.3".into(),group_id:"backend".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let forward = Forward{id:"f".into(),name:"tcp".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:443,allowed_group_ids:vec!["clients".into()]};
        let secret = StaticSecret::from([151u8;32]);
        let hub_public = PublicKey::from(&StaticSecret::from([152u8;32]));
        let endpoint_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let endpoint = endpoint_socket.local_addr().unwrap();
        let mut peers = HashMap::new();
        peers.insert("b".into(), RuntimePeer{peer:backend.clone(),group:None,tunnel:Tunn::new(secret,hub_public,None,None,1,None),endpoint:Some(endpoint),last_data_unix:None,receiver_index:256});
        let mut nat = Flows::new(hub_ip, &[forward.clone()]);
        let now = Instant::now();
        let mut out = vec![0;65535];

        for (sport, dport) in [(12001,443),(12002,8443)] {
            let f = Forward{target_port:dport,..forward.clone()};
            let raw = service_packet(6,[10,88,0,2],[10,88,0,1],sport,dport,0x02,b"secret-tcp-payload");
            let validated = ipv4::validate_forwarded(&raw).expect("valid TCP fixture");
            let (bytes, reservation) = nat.prepare_forward_packet(&validated,&source,&f,&backend,now).expect("TCP SYN reserves a flow");
            assert_packet_checksums(&bytes);
            assert_eq!(&bytes[12..16],&[10,88,0,1]);
            assert_eq!(&bytes[16..20],&[10,88,0,3]);
            assert_eq!(u16::from_be_bytes([bytes[22],bytes[23]]),dport);
            assert_ne!(u16::from_be_bytes([bytes[20],bytes[21]]),sport,"each source tuple receives a translated source port");
            let mut plan = DeliveryPlan{source_id:"a".into(),target_id:"b".into(),bytes,reservation:Some(reservation),forward_id:Some(f.id.clone())};
            let outcome = deliver_plan(&endpoint_socket,&mut peers,&plan,&mut out).await;
            assert_eq!(outcome,EgressOutcome::NotReady,"cold TCP must remain application-owned until PacketData");
            complete_delivery(&mut nat,&mut peers,&mut plan,outcome);
            assert_eq!(nat.test_state_counts(),(0,0),"cold TCP does not commit or leave a reservation");
            assert_eq!(peers["b"].peer.sent_bytes,0,"handshake traffic is not application accounting");
            if sport==12001 {
                let (n,_) = timeout(Duration::from_secs(1),endpoint_socket.recv_from(&mut out)).await.unwrap().unwrap();
                assert!(!out[..n].windows(b"secret-tcp-payload".len()).any(|w|w==b"secret-tcp-payload"),"plaintext must never reach the network socket");
            } else {
                assert!(timeout(Duration::from_millis(100),endpoint_socket.recv_from(&mut out)).await.is_err(),"one in-progress handshake is shared and no second SYN plaintext is emitted");
            }
        }
    }

    #[test]
    fn pending_forward_tcp_is_retained_only_while_current_policy_authorizes_it() {
        let secret_a=StaticSecret::from([181u8;32]);
        let secret_b=StaticSecret::from([182u8;32]);
        let public_a=base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret_a).as_bytes());
        let public_b=base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret_b).as_bytes());
        let source=Peer{id:"a".into(),name:"a".into(),public_key:public_a.clone(),ipv4:"10.88.0.2".into(),group_id:"clients".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let target=Peer{id:"b".into(),name:"b".into(),public_key:public_b.clone(),ipv4:"10.88.0.3".into(),group_id:"backend".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let source_group=Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]};
        let target_group=Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]};
        let forward=Forward{id:"f".into(),name:"tcp".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:443,allowed_group_ids:vec!["clients".into()]};
        let hub_public=PublicKey::from(&StaticSecret::from([183u8;32]));
        let runtime=|peer:Peer,group:Group| RuntimePeer{peer,group:Some(group),tunnel:Tunn::new(StaticSecret::from([184u8;32]),hub_public,None,None,1,None),endpoint:None,last_data_unix:None,receiver_index:256};
        let mut peers=HashMap::new();peers.insert("a".into(),runtime(source,source_group.clone()));peers.insert("b".into(),runtime(target,target_group));
        let packet=ipv4::validate_forwarded(&service_packet(6,[10,88,0,2],[10,88,0,1],13000,443,0x02,b"queued")).unwrap();
        let mut queue=VecDeque::new();let mut queued_bytes=0;
        enqueue_pending(&mut queue,&mut queued_bytes,PendingDelivery{source_id:"a".into(),source_key:public_a,source_ip:packet.src(),packet,reply_only:false,deadline:Instant::now()+PENDING_TTL,target_id:"b".into(),target_key:public_b,target_ip:"10.88.0.3".parse().unwrap(),forward_id:Some("f".into()),forward_protocol:Some("tcp".into()),forward_target_port:Some(443)});
        assert_eq!(queue.len(),1,"cold target handshake leaves the original packet application-owned");

        // An unrelated reload keeps the queued delivery eligible; no handshake
        // or PacketData is needed to evaluate the authorization decision.
        let mut nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[forward.clone()]);
        retain_pending(&mut queue,&mut queued_bytes,&peers,&[forward.clone()],Some(Ipv4Addr::new(10,88,0,1)),&nat);
        assert_eq!(queue.len(),1,"unrelated reload retains an authorized queued SYN");

        // Model an ACL revocation in the acknowledged snapshot. The cold queue
        // is discarded before a later target handshake can drain it.
        peers.get_mut("a").unwrap().group.as_mut().unwrap().allowed_groups.clear();
        nat.reconcile(Some(Ipv4Addr::new(10,88,0,1)),Some(Ipv4Addr::new(10,88,0,1)),&[forward.clone()],&[forward.clone()],|id| peers.get(id).map(RuntimePeer::policy));
        retain_pending(&mut queue,&mut queued_bytes,&peers,&[forward],Some(Ipv4Addr::new(10,88,0,1)),&nat);
        assert!(queue.is_empty(),"revoked queued SYN must be purged before target handshake");
        assert_eq!(queued_bytes,0);
        assert_eq!(nat.test_state_counts(),(0,0),"revoked cold SYN grants no forward or reverse flow");
    }

    fn pending_forward_fixture() -> (HashMap<String, RuntimePeer>, Forward, ipv4::ValidatedPacket) {
        let secret_a=StaticSecret::from([191u8;32]);
        let secret_b=StaticSecret::from([192u8;32]);
        let public_a=base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret_a).as_bytes());
        let public_b=base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret_b).as_bytes());
        let source=Peer{id:"a".into(),name:"a".into(),public_key:public_a,ipv4:"10.88.0.2".into(),group_id:"clients".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let target=Peer{id:"b".into(),name:"b".into(),public_key:public_b,ipv4:"10.88.0.3".into(),group_id:"backend".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let source_group=Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]};
        let target_group=Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]};
        let hub_public=PublicKey::from(&StaticSecret::from([193u8;32]));
        let runtime=|peer:Peer,group:Group| RuntimePeer{peer,group:Some(group),tunnel:Tunn::new(StaticSecret::from([194u8;32]),hub_public,None,None,1,None),endpoint:None,last_data_unix:None,receiver_index:256};
        let mut peers=HashMap::new();peers.insert("a".into(),runtime(source,source_group));peers.insert("b".into(),runtime(target,target_group));
        let forward=Forward{id:"f".into(),name:"tcp".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:443,allowed_group_ids:vec!["clients".into()]};
        let packet=ipv4::validate_forwarded(&service_packet(6,[10,88,0,2],[10,88,0,1],13000,443,0x02,b"queued")).unwrap();
        (peers,forward,packet)
    }

    fn queued_forward(packet: ipv4::ValidatedPacket, peers: &HashMap<String,RuntimePeer>, forward: &Forward, deadline: Instant) -> PendingDelivery {
        PendingDelivery{source_id:"a".into(),source_key:peers["a"].peer.public_key.clone(),source_ip:packet.src(),packet,reply_only:false,deadline,target_id:"b".into(),target_key:peers["b"].peer.public_key.clone(),target_ip:peers["b"].peer.ipv4.parse().unwrap(),forward_id:Some(forward.id.clone()),forward_protocol:Some(forward.protocol.clone()),forward_target_port:Some(forward.target_port)}
    }

    #[test]
    fn pending_forward_purges_when_source_ip_changes_without_identity_change() {
        let (mut peers,forward,packet)=pending_forward_fixture();
        let deadline=Instant::now()+PENDING_TTL;
        let mut queue=VecDeque::from([queued_forward(packet,&peers,&forward,deadline)]);let mut bytes=queue[0].packet.bytes().len();
        peers.get_mut("a").unwrap().peer.ipv4="10.88.0.22".into();
        let nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[forward.clone()]);
        retain_pending(&mut queue,&mut bytes,&peers,&[forward],Some(Ipv4Addr::new(10,88,0,1)),&nat);
        assert!(queue.is_empty(),"pending packet source IP is part of its authenticated identity");
        assert_eq!(bytes,0);
    }

    #[test]
    fn pending_forward_purges_when_target_ip_changes_without_identity_change() {
        let (mut peers,forward,packet)=pending_forward_fixture();
        let deadline=Instant::now()+PENDING_TTL;
        let mut queue=VecDeque::from([queued_forward(packet,&peers,&forward,deadline)]);let mut bytes=queue[0].packet.bytes().len();
        peers.get_mut("b").unwrap().peer.ipv4="10.88.0.33".into();
        let nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[forward.clone()]);
        retain_pending(&mut queue,&mut bytes,&peers,&[forward],Some(Ipv4Addr::new(10,88,0,1)),&nat);
        assert!(queue.is_empty(),"pending packet target IP is part of its authenticated identity");
        assert_eq!(bytes,0);
    }

    #[test]
    fn pending_forward_purges_when_same_id_forward_protocol_or_port_changes() {
        for changed in [Forward{protocol:"udp".into(),..forward_for_identity_test()},Forward{target_port:8443,..forward_for_identity_test()}] {
            let (peers,original,packet)=pending_forward_fixture();
            let deadline=Instant::now()+PENDING_TTL;
            let mut queue=VecDeque::from([queued_forward(packet,&peers,&original,deadline)]);let mut bytes=queue[0].packet.bytes().len();
            let nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[changed.clone()]);
            retain_pending(&mut queue,&mut bytes,&peers,&[changed],Some(Ipv4Addr::new(10,88,0,1)),&nat);
            assert!(queue.is_empty(),"forward protocol and target port are part of pending identity");
            assert_eq!(bytes,0);
        }
    }

    fn forward_for_identity_test() -> Forward { Forward{id:"f".into(),name:"tcp".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:443,allowed_group_ids:vec!["clients".into()]} }

    #[test]
    fn pending_forward_unchanged_identity_preserves_exact_deadline() {
        let (peers,forward,packet)=pending_forward_fixture();
        let deadline=Instant::now()+PENDING_TTL;
        let mut queue=VecDeque::from([queued_forward(packet,&peers,&forward,deadline)]);let mut bytes=queue[0].packet.bytes().len();
        let nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[forward.clone()]);
        retain_pending(&mut queue,&mut bytes,&peers,&[forward],Some(Ipv4Addr::new(10,88,0,1)),&nat);
        assert_eq!(queue.len(),1);
        assert_eq!(queue[0].deadline,deadline,"unrelated reload must not extend or shorten original deadline");
    }

    #[tokio::test]
    async fn drain_pending_releases_reservation_when_original_target_ip_is_reassigned() {
        let (mut peers,_forward,_)=pending_forward_fixture();
        let packet=ipv4::validate(&service_packet(17,[10,88,0,2],[10,88,0,3],13000,443,0,b"queued"),&peers["a"].peer,peers["a"].group.as_ref()).unwrap();
        let mut moved=peers["b"].peer.clone();moved.ipv4="10.88.0.33".into();
        peers.get_mut("b").unwrap().peer=moved;
        let secret_c=StaticSecret::from([195u8;32]);
        let c=Peer{id:"c".into(),name:"c".into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret_c).as_bytes()),ipv4:"10.88.0.3".into(),group_id:"backend".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let hub_public=PublicKey::from(&StaticSecret::from([196u8;32]));
        peers.insert("c".into(),RuntimePeer{peer:c,group:Some(Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]}),tunnel:Tunn::new(secret_c,hub_public,None,None,1,None),endpoint:None,last_data_unix:None,receiver_index:512});
        let original_b=Peer{ipv4:"10.88.0.3".into(),..peers["b"].peer.clone()};
        let now=Instant::now();
        let mut nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[]);
        let (_,reservation)=nat.prepare_direct(&packet,&peers["a"].peer,&original_b,now).expect("original target reservation");
        // Keep the reservation pending, as it is while an asynchronous delivery is being drained.
        std::mem::forget(reservation);
        let item=PendingDelivery{source_id:"a".into(),source_key:peers["a"].peer.public_key.clone(),source_ip:packet.src(),packet,reply_only:false,deadline:now+PENDING_TTL,target_id:"b".into(),target_key:original_b.public_key.clone(),target_ip:"10.88.0.3".parse().unwrap(),forward_id:None,forward_protocol:None,forward_target_port:None};
        let mut queue=VecDeque::from([item]);let mut queued_bytes=queue[0].packet.bytes().len();
        let socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();let mut out=vec![0;65535];
        drain_pending(&socket,&mut queue,&mut queued_bytes,&mut peers,&[],Some(Ipv4Addr::new(10,88,0,1)),&mut nat,&mut out).await;
        assert!(queue.is_empty());
        assert_eq!(queued_bytes,0);
        assert_eq!(nat.test_state_counts(),(0,0),"rejected retarget plan must release every active or pending flow reservation");
    }

    #[tokio::test]
    async fn drain_pending_releases_new_retarget_plan_with_empty_flow_state() {
        let (mut peers,_,_)=pending_forward_fixture();
        let packet=ipv4::validate(&service_packet(17,[10,88,0,2],[10,88,0,3],13001,444,0,b"queued"),&peers["a"].peer,peers["a"].group.as_ref()).unwrap();
        let captured_target=peers["b"].peer.clone();
        peers.get_mut("b").unwrap().peer.ipv4="10.88.0.33".into();
        let secret_c=StaticSecret::from([197u8;32]);
        let c=Peer{id:"c".into(),name:"c".into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret_c).as_bytes()),ipv4:"10.88.0.3".into(),group_id:"backend".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let hub_public=PublicKey::from(&StaticSecret::from([198u8;32]));
        peers.insert("c".into(),RuntimePeer{peer:c,group:Some(Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]}),tunnel:Tunn::new(secret_c,hub_public,None,None,1,None),endpoint:None,last_data_unix:None,receiver_index:512});
        let item=PendingDelivery{source_id:"a".into(),source_key:peers["a"].peer.public_key.clone(),source_ip:packet.src(),packet,reply_only:false,deadline:Instant::now()+PENDING_TTL,target_id:"b".into(),target_key:captured_target.public_key,target_ip:"10.88.0.3".parse().unwrap(),forward_id:None,forward_protocol:None,forward_target_port:None};
        let mut queue=VecDeque::from([item]);let mut queued_bytes=queue[0].packet.bytes().len();
        let mut nat=Flows::new(Ipv4Addr::new(10,88,0,1),&[]);
        assert_eq!(nat.test_state_counts(),(0,0),"this case starts without an orphaned pending reservation");
        let counters_before:HashMap<_,_>=peers.iter().map(|(id,p)|(id.clone(),(p.peer.received_bytes,p.peer.sent_bytes))).collect();
        let socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();let mut out=vec![0;65535];
        drain_pending(&socket,&mut queue,&mut queued_bytes,&mut peers,&[],Some(Ipv4Addr::new(10,88,0,1)),&mut nat,&mut out).await;
        assert!(queue.is_empty());
        assert_eq!(queued_bytes,0);
        assert_eq!(nat.test_state_counts(),(0,0),"the fresh plan for replacement C is rejected against captured B provenance and released");
        assert_eq!(peers.iter().map(|(id,p)|(id.clone(),(p.peer.received_bytes,p.peer.sent_bytes))).collect::<HashMap<_,_>>(),counters_before);
    }

    #[tokio::test]
    async fn invalid_mac_initiation_shapes_do_not_reach_anonymous_parse() {
        let store = Arc::new(Store::open(":memory:").unwrap());
        store.bind_test_identity();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let (commands, receiver) = mpsc::channel(1);
        let (ready, started) = oneshot::channel();
        tokio::spawn(run_udp(socket,store,[161u8;32],receiver,RuntimeStats::default(),Readiness::default(),Some(ready)));
        started.await.unwrap().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let before = ANON_PARSE_COUNT.with(|count| count.load(Ordering::SeqCst));
        let mut malformed = [0u8;148];
        malformed[..4].copy_from_slice(&1u32.to_le_bytes());
        sender.send_to(&malformed,address).await.unwrap();
        time::sleep(Duration::from_millis(100)).await;
        let parsed = ANON_PARSE_COUNT.with(|count| count.load(Ordering::SeqCst)) - before;
        assert_eq!(parsed,0,"invalid-MAC initiation must be rejected before anonymous parsing");
        drop(commands);
    }

    #[tokio::test]
    async fn reload_revocation_blocks_established_direct_flow_requests_and_replies() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("cold-revoke.sqlite").to_str().unwrap()).unwrap());
        store.bind_test_identity();store.setup("10.89.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"a".into(),name:"A".into(),allowed_groups:vec!["b".into()]}).unwrap();
        store.add_group(&Group{id:"b".into(),name:"B".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("a", &["b".into()]).unwrap();
        let a_secret=StaticSecret::from([91u8;32]); let b_secret=StaticSecret::from([92u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.89.0.2","a"),("b",&b_secret,"10.89.0.3","b")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        let hub_private=[93u8;32]; let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(2); let (ready,ready_rx)=oneshot::channel(); let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Readiness::default(),Some(ready))); ready_rx.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret.clone(),hub_public,None,None,91,None); let mut b=Tunn::new(b_secret.clone(),hub_public,None,None,92,None);
        let mut tx=vec![0;65535]; let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await; establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;
        let request=service_packet(17,[10,89,0,2],[10,89,0,3],2345,9090,0,b"before-revoke");
        send_inner(&a_socket,address,&mut a,&request,&mut tx).await;
        assert_eq!(&recv_inner(&b_socket,&mut b,&mut rx,&mut tx).await[28..],b"before-revoke");

        // Revocation removes established state before acknowledging policy.
        store.set_acl("a", &[]).unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        let reply=service_packet(17,[10,89,0,3],[10,89,0,2],9090,2345,0,b"revoked-reply");
        send_inner(&b_socket,address,&mut b,&reply,&mut tx).await;
        assert_no_inner(&a_socket,address,&mut a,&mut rx,&mut tx).await;
        send_inner(&a_socket,address,&mut a,&service_packet(17,[10,89,0,2],[10,89,0,3],2345,9090,0,b"after-revoke"),&mut tx).await;
        assert_no_inner(&b_socket,address,&mut b,&mut rx,&mut tx).await;
        let snapshot=stats.read().await;
        assert_eq!(snapshot["a"].0,request.len() as u64,"revoked traffic is not counted as delivered ingress");
        assert_eq!(snapshot["b"].1,request.len() as u64,"revoked traffic is not counted as delivered egress");
    }

    #[test]
    fn expired_queued_reverse_reply_cannot_fall_back_to_acl_direct_route() {
        // Anchor synthetic flow time in the past so the queue deadline remains
        // live at the simulated 61-second flow-expiry check.
        let now=Instant::now()-Duration::from_secs(61);
        let a=Peer{id:"a".into(),name:"a".into(),public_key:String::new(),ipv4:"10.90.0.2".into(),group_id:"a".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let b=Peer{id:"b".into(),name:"b".into(),public_key:String::new(),ipv4:"10.90.0.3".into(),group_id:"b".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let ga=Group{id:"a".into(),name:"a".into(),allowed_groups:vec!["b".into()]};
        let gb=Group{id:"b".into(),name:"b".into(),allowed_groups:vec!["a".into()]};
        let mut peers=HashMap::new();
        for (peer,group,key) in [(a.clone(),ga.clone(),[101u8;32]),(b.clone(),gb.clone(),[102u8;32])] {
            let tunnel=Tunn::new(StaticSecret::from(key),PublicKey::from(&StaticSecret::from([103u8;32])),None,None,1,None);
            peers.insert(peer.id.clone(),RuntimePeer{peer,group:Some(group),tunnel,endpoint:None,last_data_unix:None,receiver_index:256});
        }
        let mut nat=Flows::new(Ipv4Addr::new(10,90,0,1),&[]);
        let original=ipv4::validate(&service_packet(17,[10,90,0,2],[10,90,0,3],1234,9090,0,b"request"),&a,Some(&ga)).unwrap();
        let (_bytes,reservation)=nat.prepare_direct(&original,&a,&peers["b"].peer,now).unwrap();
        nat.complete(reservation,true,now);
        let reply=ipv4::validate(&service_packet(17,[10,90,0,3],[10,90,0,2],9090,1234,0,b"reply"),&b,Some(&gb)).unwrap();
        // The independent ACL permits B->A direct traffic, so generic resolution
        // would produce a direct plan once the reverse-flow entry expires.
        assert!(resolve_packet(&reply,&b,&gb,"b",&peers,&[],Some(Ipv4Addr::new(10,90,0,1)),&mut nat,now+Duration::from_secs(61)).0.is_some());

        // Give this manually constructed queue item a longer deadline so the
        // synthetic timestamp isolates reverse-flow expiry from queue expiry.
        let queued=PendingDelivery{source_id:"b".into(),source_key:b.public_key.clone(),source_ip:reply.src(),packet:reply,reply_only:true,deadline:now+Duration::from_secs(120),target_id:"a".into(),target_key:a.public_key.clone(),target_ip:a.ipv4.parse().unwrap(),forward_id:None,forward_protocol:None,forward_target_port:None};
        let at=now+Duration::from_secs(61);
        assert!(!queued.expired_at(at),"test advances NAT expiry while keeping queue deadline live");
        assert!(nat.lookup_reply(&queued.packet,&b,at).is_none(),"reverse mapping has expired");
        // This is the drain_pending reply_only branch: it must only consult the
        // reverse mapping, never call resolve_packet and re-route by ACL.
        let queued_reply_plan=if queued.reply_only {
            nat.lookup_reply(&queued.packet,&b,at).map(|(target_id,bytes,reservation)|DeliveryPlan{source_id:queued.source_id.clone(),target_id,bytes,reservation:Some(reservation),forward_id:None})
        } else {
            resolve_packet(&queued.packet,&b,&gb,"b",&peers,&[],Some(Ipv4Addr::new(10,90,0,1)),&mut nat,at).0
        };
        assert!(queued_reply_plan.is_none(),"expired queued reverse reply cannot use an ACL-permitted direct route");
    }

    #[test]
    fn icmp_direct_route_uses_runtime_acl_without_nat_reservation() {
        let a_secret = StaticSecret::from([221u8; 32]);
        let b_secret = StaticSecret::from([222u8; 32]);
        let hub_secret = StaticSecret::from([223u8; 32]);
        let mut peers = HashMap::new();
        for (id, ip, group_id, secret, allowed_groups, index) in [
            ("a", "10.77.0.2", "a", &a_secret, vec!["b".to_string()], 1),
            ("b", "10.77.0.3", "b", &b_secret, Vec::new(), 2),
        ] {
            let peer = Peer {
                id: id.into(), name: id.into(),
                public_key: base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),
                ipv4: ip.into(), group_id: group_id.into(), received_bytes: 0, sent_bytes: 0,
                last_handshake_unix: None,
            };
            peers.insert(id.into(), RuntimePeer {
                peer,
                group: Some(Group { id: group_id.into(), name: group_id.into(), allowed_groups }),
                tunnel: Tunn::new(hub_secret.clone(), PublicKey::from(secret), None, None, index, None),
                endpoint: None, last_data_unix: None, receiver_index: index << 8,
            });
        }

        let echo = |src: [u8; 4], dst: [u8; 4], kind: u8| {
            let mut packet = vec![0; 28];
            packet[0] = 0x45;
            packet[2..4].copy_from_slice(&28u16.to_be_bytes());
            packet[8] = 64;
            packet[9] = 1;
            packet[12..16].copy_from_slice(&src);
            packet[16..20].copy_from_slice(&dst);
            packet[20] = kind;
            packet[24..26].copy_from_slice(&0x1234u16.to_be_bytes());
            packet[26..28].copy_from_slice(&1u16.to_be_bytes());
            let icmp_checksum = checksum(&packet[20..]);
            packet[22..24].copy_from_slice(&icmp_checksum.to_be_bytes());
            let ip_checksum = checksum(&packet[..20]);
            packet[10..12].copy_from_slice(&ip_checksum.to_be_bytes());
            packet
        };

        let now = Instant::now();
        let mut nat = Flows::new("10.77.0.1".parse().unwrap(), &[]);
        let b = peers["b"].peer.clone();
        let b_group = peers["b"].group.clone().unwrap();
        let request_raw = echo([10, 77, 0, 3], [10, 77, 0, 2], 8);
        let request = ipv4::validate(&request_raw, &b, Some(&b_group)).expect("B-owned valid ICMP request");
        assert_eq!(request.src(), "10.77.0.3".parse::<Ipv4Addr>().unwrap());
        let (denied, _) = resolve_packet(&request, &b, &b_group, "b", &peers, &[], None, &mut nat, now);
        assert!(denied.is_none(), "B cannot route to A before its directed ACL grant");

        peers.get_mut("b").unwrap().group.as_mut().unwrap().allowed_groups.push("a".into());
        let b_group = peers["b"].group.clone().unwrap();
        let (plan, _) = resolve_packet(&request, &b, &b_group, "b", &peers, &[], None, &mut nat, now);
        let plan = plan.expect("runtime B-to-A ACL grant permits reservation-free ICMP plan");
        assert_eq!(plan.target_id, "a");
        assert!(plan.reservation.is_none(), "ICMP direct routing must not allocate a flow reservation");
        let mut expected_request = request_raw.clone();
        expected_request[8] = 63;
        expected_request[10..12].fill(0);
        let expected_ip_checksum = checksum(&expected_request[..20]);
        expected_request[10..12].copy_from_slice(&expected_ip_checksum.to_be_bytes());
        assert_eq!(plan.bytes, expected_request, "only IPv4 TTL/checksum change, exactly once");

        let a = peers["a"].peer.clone();
        let a_group = peers["a"].group.clone().unwrap();
        let reply_raw = echo([10, 77, 0, 2], [10, 77, 0, 3], 0);
        let reply = ipv4::validate(&reply_raw, &a, Some(&a_group)).expect("A-owned valid ICMP echo reply");
        let (reply_plan, _) = resolve_packet(&reply, &a, &a_group, "a", &peers, &[], None, &mut nat, now);
        let reply_plan = reply_plan.expect("A-to-B reply remains directly routable");
        assert_eq!(reply_plan.target_id, "b");
        assert!(reply_plan.reservation.is_none());
        assert_eq!(nat.test_state_counts(), (0, 0), "ICMP exchanges leave no active or pending flows");
    }

#[test]
fn established_tcp_survives_reload_but_acl_revocation_removes_flow_and_queued_icmp_immediately() {
    let (mut peers,forward,_)=pending_forward_fixture();let hub=Ipv4Addr::new(10,88,0,1);let now=Instant::now();
    let tcp_packet=|src:[u8;4],dst:[u8;4],sport,dport,flags,seq:u32,ack:u32| {
        let mut raw=service_packet(6,src,dst,sport,dport,flags,b"");
        raw[24..28].copy_from_slice(&seq.to_be_bytes());raw[28..32].copy_from_slice(&ack.to_be_bytes());raw[36..38].fill(0);
        let sum=transport_checksum(src,dst,6,&raw[20..]);raw[36..38].copy_from_slice(&sum.to_be_bytes());
        ipv4::validate_forwarded(&raw).unwrap()
    };
    let a=&peers["a"].peer;let b=&peers["b"].peer;let mut nat=Flows::new(hub,&[forward.clone()]);
    let syn=tcp_packet([10,88,0,2],hub.octets(),1234,443,2,100,0);
    let (translated,r)=nat.prepare_forward_packet(&syn,a,&forward,b,now).unwrap();nat.complete(r,true,now);
    let snat=u16::from_be_bytes([translated[20],translated[21]]);
    let syn_ack=tcp_packet([10,88,0,3],hub.octets(),443,snat,0x12,200,101);
    let (_,_,r)=nat.lookup_reply(&syn_ack,b,now).unwrap();nat.complete(r,true,now);
    // Later ACK/data also confirms establishment when the original final ACK was lost.
    let ack=tcp_packet([10,88,0,2],hub.octets(),1234,443,0x10,110,201);
    let (_,r)=nat.prepare_forward_packet(&ack,a,&forward,b,now).unwrap();nat.complete(r,true,now);
    nat.reconcile(Some(hub),Some(hub),&[forward.clone()],&[forward.clone()],|id| peers.get(id).map(RuntimePeer::policy));
    nat.expire(now+Duration::from_secs(301));assert_eq!(nat.test_state_counts(),(1,0));
    let raw=ipv4::test_icmp_error(b.ipv4.parse().unwrap(),hub,3,4,&translated[..28]);
    let error=ipv4::validate(&raw,b,peers["b"].group.as_ref()).unwrap();
    let (plan,reply_only)=resolve_packet(&error,b,peers["b"].group.as_ref().unwrap(),"b",&peers,&[forward.clone()],Some(hub),&mut nat,now);
    assert!(reply_only);let mut plan=plan.unwrap();assert_eq!(plan.target_id,"a");
    complete_delivery(&mut nat,&mut peers,&mut plan,EgressOutcome::NotReady);
    let pending=PendingDelivery { source_id:"b".into(),source_key:peers["b"].peer.public_key.clone(),source_ip:peers["b"].peer.ipv4.parse().unwrap(),packet:error.clone(),reply_only:true,deadline:now+PENDING_TTL,target_id:"a".into(),target_key:peers["a"].peer.public_key.clone(),target_ip:peers["a"].peer.ipv4.parse().unwrap(),forward_id:None,forward_protocol:None,forward_target_port:None };
    let mut queue=VecDeque::from([pending]);let mut bytes=raw.len();
    retain_pending(&mut queue,&mut bytes,&peers,&[forward.clone()],Some(hub),&nat);assert_eq!(queue.len(),1);
    peers.get_mut("a").unwrap().group.as_mut().unwrap().allowed_groups.clear();
    nat.reconcile(Some(hub),Some(hub),&[forward.clone()],&[forward.clone()],|id| peers.get(id).map(RuntimePeer::policy));
    retain_pending(&mut queue,&mut bytes,&peers,&[forward.clone()],Some(hub),&nat);
    assert_eq!(nat.test_state_counts(),(0,0));assert!(queue.is_empty());assert_eq!(bytes,0);
    let (plan,_)=resolve_packet(&error,&peers["b"].peer,peers["b"].group.as_ref().unwrap(),"b",&peers,&[forward],Some(hub),&mut nat,now);
    assert!(plan.is_none(),"ICMP errors cannot fall back to ACL routing after revocation");
}
