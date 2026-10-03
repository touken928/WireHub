 use super::*; use rusqlite::Connection;
 fn group(id:&str)->Group{Group{id:id.into(),name:id.into(),allowed_groups:vec![]}}
 fn peer(id:&str)->Peer{Peer{id:id.into(),name:id.into(),public_key:format!("key-{id}"),ipv4:String::new(),group_id:"group".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}}
 fn forward(id:&str)->Forward{Forward{id:id.into(),name:id.into(),protocol:"tcp".into(),target_peer_id:"peer".into(),target_port:9000,allowed_group_ids:vec!["group".into()]}}
 fn store()->Store{let s=Store::open(":memory:").unwrap();s.bind_test_identity();s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();s}
   #[test] fn setup_once_persists_and_allocations_are_disjoint_transactional(){let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db");let s=Store::open(path.to_str().unwrap()).unwrap();s.bind_test_identity();assert!(s.create_peer_allocated(&mut peer("no-settings")).is_err());assert!(s.add_group(&group("no-setup")).is_err());assert_eq!(s.network_settings().unwrap(),None);let settings=s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();assert_eq!(settings.subnet,"10.77.0.0/24");assert!(s.setup("10.77.0.0/24","hub.example:51820",25).is_err());drop(s);let s=Store::open(path.to_str().unwrap()).unwrap();assert_eq!(s.network_settings().unwrap().unwrap().endpoint,"hub.example:51820");s.add_group(&group("group")).unwrap();let mut p=peer("peer");assert!(s.create_peer_allocated(&mut p).unwrap());assert_eq!(p.ipv4,"10.77.0.2");let mut f=forward("f");s.create_forward(&mut f).unwrap();assert_eq!(s.forwards().unwrap().len(),1);}
   #[test] fn runtime_snapshot_reads_all_inventory_and_fails_as_a_unit(){
       let s=store(); s.add_group(&group("group")).unwrap();
       let mut p=peer("peer"); assert!(s.create_peer_allocated(&mut p).unwrap());
       let mut f=forward("forward"); s.create_forward(&mut f).unwrap();
       let snapshot=s.runtime_snapshot().unwrap();
       assert_eq!(snapshot.settings.unwrap().endpoint,"hub.example:51820");
       assert_eq!(snapshot.groups.len(),1); assert_eq!(snapshot.groups[0].id,"group");
       assert_eq!(snapshot.peers.len(),1); assert_eq!(snapshot.peers[0].id,"peer");
       assert_eq!(snapshot.forwards.len(),1); assert_eq!(snapshot.forwards[0].id,"forward");

       s.db.lock().unwrap().execute("UPDATE groups SET allowed='not-json'",[]).unwrap();
       assert!(s.runtime_snapshot().is_err());
   }
  #[test] fn peer_pool_allocates_all_addresses_then_exhausts_and_reuses_released_address(){let s=store();s.add_group(&group("group")).unwrap();for offset in 2..=254{let id=format!("peer-{offset}");let mut p=peer(&id);assert!(s.create_peer_allocated(&mut p).unwrap());assert_eq!(p.ipv4,format!("10.77.0.{offset}"));}assert_eq!(s.peers().unwrap().len(),253);let mut exhausted=peer("exhausted");assert!(!s.create_peer_allocated(&mut exhausted).unwrap());assert_eq!(exhausted.ipv4,"");s.remove_peer("peer-2").unwrap();let mut replacement=peer("replacement");assert!(s.create_peer_allocated(&mut replacement).unwrap());assert_eq!(replacement.ipv4,"10.77.0.2");}
 #[test] fn settings_update_is_atomic_and_keeps_subnet_immutable(){let s=store();let before=s.network_settings().unwrap().unwrap();assert!(s.update_settings("bad:0",25).is_err());assert_eq!(s.network_settings().unwrap().unwrap(),before);let updated=s.update_settings("vpn.example:443",0).unwrap();assert_eq!(updated.subnet,before.subnet);assert_eq!(updated.endpoint,"vpn.example:443");assert_eq!(updated.persistent_keepalive,0);}
 #[test] fn names_are_trimmed_and_unique(){let s=store();s.add_group(&Group{name:" Team ".into(),..group("g")}).unwrap();assert_eq!(s.group("g").unwrap().unwrap().name,"Team");assert!(s.add_group(&Group{name:"Team".into(),..group("g2")}).is_err());assert!(s.add_group(&Group{name:"  ".into(),..group("g3")}).is_err());s.add_group(&group("users")).unwrap();s.add_group(&group("group")).unwrap();let mut p=peer("p1");p.name=" Alice ".into();assert!(s.create_peer_allocated(&mut p).unwrap());assert_eq!(s.peers().unwrap()[0].name,"Alice");let mut duplicate=peer("p2");duplicate.name="Alice".into();assert!(s.create_peer_allocated(&mut duplicate).is_err());let mut empty=peer("p3");empty.name="   ".into();assert!(s.create_peer_allocated(&mut empty).is_err());}
  #[test] fn fresh_database_initializes_current_schema(){let d=tempfile::tempdir().unwrap();let p=d.path().join("fresh");let s=Store::open(p.to_str().unwrap()).unwrap();assert_eq!(s.network_settings().unwrap(),None);drop(s);let db=Connection::open(&p).unwrap();assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),SCHEMA_VERSION);for table in ["groups","peers","forwards","network_settings"]{let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",[table],|r|r.get(0)).unwrap();assert!(exists);}}
  #[test] fn forward_uniqueness_is_protocol_and_target_port_and_has_no_pool_limit(){let s=store();s.add_group(&group("group")).unwrap();for id in ["peer","peer2"]{let mut p=peer(id);s.create_peer_allocated(&mut p).unwrap();}let mut tcp=forward("one");tcp.target_peer_id="peer".into();tcp.target_port=443;s.create_forward(&mut tcp).unwrap();let mut udp=Forward{protocol:"udp".into(),..forward("udp")};udp.target_port=443;s.create_forward(&mut udp).unwrap();let mut dup=Forward{id:"dup".into(),target_peer_id:"peer2".into(),..forward("dup")};dup.target_port=443;let error=s.create_forward(&mut dup).unwrap_err();assert!(matches!(error,rusqlite::Error::SqliteFailure(ref e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation));for port in 1..=140 {let mut f=Forward{id:format!("f{port}"),protocol:"tcp".into(),target_peer_id:"peer".into(),target_port:1000+port, ..forward("many")};s.create_forward(&mut f).unwrap();}assert_eq!(s.forwards().unwrap().len(),142);}
  #[test] fn rejects_v1_unchanged_including_empty_database(){for populated in [false,true]{let d=tempfile::tempdir().unwrap();let p=d.path().join("v1");let db=Connection::open(&p).unwrap();if populated{db.execute_batch("CREATE TABLE old(value TEXT); INSERT INTO old VALUES('keep')").unwrap();}db.pragma_update(None,"user_version",1).unwrap();drop(db);let before=std::fs::read(&p).unwrap();let error=Store::open(p.to_str().unwrap()).err().unwrap();assert!(error.to_string().contains("including v1"));assert_eq!(std::fs::read(&p).unwrap(),before);}}
 fn assert_v0_rejected_unchanged(path:&std::path::Path){let before=std::fs::read(path).unwrap();let err=Store::open(path.to_str().unwrap()).err().unwrap();assert!(err.to_string().contains("unversioned database"));assert_eq!(std::fs::read(path).unwrap(),before);}
 #[test] fn empty_legacy_rejected_unchanged(){let d=tempfile::tempdir().unwrap();let p=d.path().join("legacy");let db=Connection::open(&p).unwrap();db.execute_batch("CREATE TABLE groups(id TEXT PRIMARY KEY,name TEXT NOT NULL)").unwrap();drop(db);assert_v0_rejected_unchanged(&p);}
 #[test] fn populated_legacy_rejected_unchanged(){let d=tempfile::tempdir().unwrap();let p=d.path().join("legacy");let db=Connection::open(&p).unwrap();db.execute_batch("CREATE TABLE groups(id TEXT PRIMARY KEY,name TEXT NOT NULL);INSERT INTO groups VALUES('g','legacy')").unwrap();drop(db);assert_v0_rejected_unchanged(&p);}
 #[test] fn version_zero_saved_settings_rejected_unchanged(){let d=tempfile::tempdir().unwrap();let p=d.path().join("settings");let db=Connection::open(&p).unwrap();db.execute_batch("CREATE TABLE network_settings(id INTEGER PRIMARY KEY,subnet TEXT,endpoint TEXT,persistent_keepalive INTEGER);INSERT INTO network_settings VALUES(1,'10.0.0.0/24','hub.example:51820',25)").unwrap();drop(db);assert_v0_rejected_unchanged(&p);}
  #[test] fn version_zero_unrelated_objects_rejected_unchanged(){let d=tempfile::tempdir().unwrap();let p=d.path().join("unrelated");let db=Connection::open(&p).unwrap();db.execute_batch("CREATE TABLE unrelated(value TEXT)").unwrap();drop(db);assert_v0_rejected_unchanged(&p);}
  #[test] fn version_zero_sqlite_like_prefix_object_rejected_unchanged(){let d=tempfile::tempdir().unwrap();let p=d.path().join("sqlite-prefix");let db=Connection::open(&p).unwrap();db.execute_batch("CREATE TABLE sqliteXlegacy(value TEXT); INSERT INTO sqliteXlegacy VALUES('preserve me')").unwrap();drop(db);assert_v0_rejected_unchanged(&p);let db=Connection::open(&p).unwrap();assert_eq!(db.query_row("SELECT value FROM sqliteXlegacy",[],|r|r.get::<_,String>(0)).unwrap(),"preserve me");assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),0);}
  fn assert_current_rejected_unchanged(path:&std::path::Path){let before=std::fs::read(path).unwrap();assert!(Store::open(path.to_str().unwrap()).is_err());assert_eq!(std::fs::read(path).unwrap(),before);let db=Connection::open(path).unwrap();assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),SCHEMA_VERSION);}
  fn current_db_with_index(index_change:&str)->(tempfile::TempDir,std::path::PathBuf){let d=tempfile::tempdir().unwrap();let p=d.path().join("current");let db=Connection::open(&p).unwrap();db.execute_batch(TABLES).unwrap();db.pragma_update(None,"user_version",SCHEMA_VERSION).unwrap();db.execute_batch(index_change).unwrap();drop(db);(d,p)}
  #[test] fn rejects_partial_peer_and_group_unique_name_indexes_unchanged(){for (table,index) in [("peers","peers_name_unique"),("groups","groups_name_unique")] {let change=format!("DROP INDEX {index}; CREATE UNIQUE INDEX {index} ON {table}(name) WHERE name <> ''; ");let (_d,p)=current_db_with_index(&change);assert_current_rejected_unchanged(&p);}}
  #[test] fn rejects_nocase_name_index_unchanged(){let (_d,p)=current_db_with_index("DROP INDEX groups_name_unique; CREATE UNIQUE INDEX groups_name_unique ON groups(name COLLATE NOCASE)");assert_current_rejected_unchanged(&p);}
    #[test] fn rejects_partial_forward_unique_index_unchanged(){let (_d,p)=current_db_with_index("DROP TABLE forwards; CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL); CREATE UNIQUE INDEX forwards_partial_unique ON forwards(protocol,target_port) WHERE target_port > 0");assert_current_rejected_unchanged(&p);}
    #[test] fn rejects_adversarial_forward_column_shape_unchanged(){let (_d,p)=current_db_with_index("DROP TABLE forwards; CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL,extra TEXT); CREATE UNIQUE INDEX forwards_unique ON forwards(protocol,target_port)");assert_current_rejected_unchanged(&p);}
    #[test] fn rejects_non_binary_forward_unique_index_unchanged(){let (_d,p)=current_db_with_index("DROP TABLE forwards; CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL); CREATE UNIQUE INDEX forwards_unique ON forwards(protocol COLLATE NOCASE,target_port)");assert_current_rejected_unchanged(&p);}
   #[test] fn rejects_expression_key_indexes_unchanged_and_raw_database_allows_duplicate_names(){
    for change in [
        "DROP INDEX groups_name_unique; CREATE UNIQUE INDEX groups_name_unique ON groups(name,length(id))",
        "DROP INDEX peers_name_unique; CREATE UNIQUE INDEX peers_name_unique ON peers(name,length(id))",
        "DROP TABLE forwards; CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL); CREATE UNIQUE INDEX forwards_expected_unique ON forwards(protocol,target_port,length(id))",
    ] { let (_d,p)=current_db_with_index(change); assert_current_rejected_unchanged(&p); }

    let (_d,p)=current_db_with_index("DROP INDEX groups_name_unique; CREATE UNIQUE INDEX groups_name_unique ON groups(name,length(id)); DROP INDEX peers_name_unique; CREATE UNIQUE INDEX peers_name_unique ON peers(name,length(id))");
    let db=Connection::open(&p).unwrap();
    db.execute_batch("INSERT INTO groups(id,name) VALUES('g1','duplicate'),('group2','duplicate'); INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES('p1','duplicate','key1','10.0.0.2','g1'),('peer22','duplicate','key2','10.0.0.3','g1')").unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM groups WHERE name='duplicate'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM peers WHERE name='duplicate'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
   }
   #[test] fn future_version_rejected_unchanged(){let d=tempfile::tempdir().unwrap();let p=d.path().join("future");let db=Connection::open(&p).unwrap();db.execute_batch("CREATE TABLE existing(value TEXT)").unwrap();db.pragma_update(None,"user_version",99).unwrap();drop(db);let before=std::fs::read(&p).unwrap();assert!(Store::open(p.to_str().unwrap()).is_err());assert_eq!(std::fs::read(&p).unwrap(),before);}
  #[test] fn exact_schema_rejects_unrelated_trigger_unchanged(){let (_d,p)=current_db_with_index("CREATE TRIGGER extra AFTER INSERT ON groups BEGIN SELECT 1; END;");assert_current_rejected_unchanged(&p);}
   #[test] fn malformed_and_dangling_persisted_group_refs_reject_database_unchanged(){for (payload,expected) in [("not-json","malformed group references"),("[\"missing\"]","unknown group references")]{let d=tempfile::tempdir().unwrap();let p=d.path().join("bad-data");let db=Connection::open(&p).unwrap();db.execute_batch(TABLES).unwrap();db.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,zeroblob(32))",[]).unwrap();db.execute("INSERT INTO groups(id,name,allowed) VALUES('g','group',?1)",[payload]).unwrap();db.pragma_update(None,"user_version",SCHEMA_VERSION).unwrap();drop(db);let error=match Store::open(p.to_str().unwrap()){Ok(_)=>panic!("invalid references unexpectedly accepted"),Err(error)=>error.to_string()};assert!(error.contains(expected),"expected {expected}, got {error}");assert_current_rejected_unchanged(&p);}}
  #[test] fn hub_identity_requires_single_strict_32_byte_blob(){let s=Store::open(":memory:").unwrap();assert!(s.bind_hub_identity(&[1;32]).is_ok());assert_eq!(s.hub_identity().unwrap(),Some([1;32]));let db=Connection::open_in_memory().unwrap();db.execute_batch(TABLES).unwrap();assert!(db.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,'text')",[]).is_err());assert!(db.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,zeroblob(31))",[]).is_err());}
   #[test] fn malformed_identity_rows_reject_open_even_when_checks_are_ignored(){let d=tempfile::tempdir().unwrap();let p=d.path().join("malformed");let db=Connection::open(&p).unwrap();db.execute_batch(TABLES).unwrap();db.pragma_update(None,"ignore_check_constraints",true).unwrap();db.execute_batch("INSERT INTO hub_identity(id,public_key) VALUES(1,'wrong-type'); PRAGMA user_version=3;").unwrap();drop(db);assert!(Store::open(p.to_str().unwrap()).is_err());}
   #[test] fn unbound_database_with_inventory_is_rejected(){let d=tempfile::tempdir().unwrap();let p=d.path().join("inventory");let db=Connection::open(&p).unwrap();db.execute_batch(TABLES).unwrap();db.execute("INSERT INTO groups(id,name) VALUES('g','g')",[]).unwrap();db.pragma_update(None,"user_version",SCHEMA_VERSION).unwrap();drop(db);assert!(Store::open(p.to_str().unwrap()).is_err());}
  #[test] fn configured_database_without_identity_and_wrong_bound_key_fail_without_mutation(){let s=Store::open(":memory:").unwrap();s.bind_test_identity();s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();let db=s.db.lock().unwrap();db.execute("DELETE FROM hub_identity",[]).unwrap();drop(db);assert!(s.hub_identity().is_err());let s=Store::open(":memory:").unwrap();s.bind_test_identity();let before=s.hub_identity().unwrap();assert!(s.bind_hub_identity(&[8;32]).is_err());assert_eq!(s.hub_identity().unwrap(),before);}
  #[test] fn invalid_group_refs_are_rejected_and_group_delete_cleans_both_allowlists(){let s=store();s.add_group(&group("group")).unwrap();s.add_group(&group("other")).unwrap();assert!(s.set_acl("group",&["missing".into()]).is_err());let mut p=peer("peer");p.group_id="other".into();s.create_peer_allocated(&mut p).unwrap();let mut f=forward("forward");assert!(s.create_forward(&mut Forward{allowed_group_ids:vec!["missing".into()],..f.clone()}).is_err());s.set_acl("other",&["group".into(),"other".into()]).unwrap();f.allowed_group_ids=vec!["group".into(),"other".into()];s.create_forward(&mut f).unwrap();assert_eq!(s.remove_group("group").unwrap(),1);assert_eq!(s.group("other").unwrap().unwrap().allowed_groups,vec!["other"]);assert_eq!(s.forwards().unwrap()[0].allowed_group_ids,vec!["other"]);}
   #[test] fn group_cleanup_rolls_back_on_malformed_forward_json_and_missing_group_is_noop(){let s=store();s.add_group(&group("group")).unwrap();s.add_group(&group("other")).unwrap();s.set_acl("other",&["group".into()]).unwrap();let mut p=peer("peer");s.create_peer_allocated(&mut p).unwrap();s.db.lock().unwrap().execute("INSERT INTO forwards(id,name,protocol,target_peer_id,target_port,allowed) VALUES('broken','broken','tcp','peer',9000,'not-json')",[]).unwrap();assert!(s.remove_group("group").is_err());assert!(s.group("group").unwrap().is_some());assert_eq!(s.group("other").unwrap().unwrap().allowed_groups,vec!["group"]);assert_eq!(s.remove_group("missing").unwrap(),0);assert_eq!(s.group("group").unwrap().unwrap().id,"group");}
  #[test] fn group_cleanup_rolls_back_when_peer_restricts_delete(){let s=store();s.add_group(&group("group")).unwrap();s.add_group(&group("other")).unwrap();s.set_acl("other",&["group".into()]).unwrap();let mut p=peer("peer");s.create_peer_allocated(&mut p).unwrap();let error=s.remove_group("group").unwrap_err();assert!(matches!(error,rusqlite::Error::SqliteFailure(ref e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation));assert_eq!(s.group("other").unwrap().unwrap().allowed_groups,vec!["group"]);assert!(s.group("group").unwrap().is_some());}
  #[test] fn bootstrap_serializes_two_connections_before_running_key_closures(){
   use std::{sync::mpsc,thread,time::Duration};
   let dir=tempfile::tempdir().unwrap();let path=dir.path().join("bootstrap.db");
   let first=std::sync::Arc::new(Store::open(path.to_str().unwrap()).unwrap());
   let second=std::sync::Arc::new(Store::open(path.to_str().unwrap()).unwrap());
   let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
   let key=[41u8;32];let first_store=first.clone();
   let first_thread=thread::spawn(move||first_store.bootstrap_hub_identity(||{entered_tx.send(()).unwrap();release_rx.recv().unwrap();Ok(key)},|_|panic!("unbound bootstrap used bound closure"),|k|Ok(*k)));
   entered_rx.recv().unwrap();
    let (closure_tx,closure_rx)=mpsc::channel();let second_store=second.clone();let(busy_tx,busy_rx)=mpsc::channel();let(busy_release_tx,busy_release_rx)=mpsc::channel();
    let second_thread=thread::spawn(move||{BUSY_TEST_HOOK.with(|h|*h.borrow_mut()=Some((busy_tx,busy_release_rx)));second_store.db.lock().unwrap().busy_handler(Some(test_busy_handler)).unwrap();second_store.bootstrap_hub_identity(||{closure_tx.send("create").unwrap();Ok([99;32])},|_|{closure_tx.send("load").unwrap();Err(std::io::Error::new(std::io::ErrorKind::NotFound,"alternate key file missing"))},|k|Ok(*k))});
    busy_rx.recv_timeout(Duration::from_secs(3)).expect("second bootstrap encountered SQLite lock");assert!(closure_rx.try_recv().is_err());
    release_tx.send(()).unwrap();assert_eq!(first_thread.join().unwrap().unwrap(),key);
    busy_release_tx.send(()).unwrap();
   assert_eq!(closure_rx.recv_timeout(Duration::from_secs(2)).unwrap(),"load");
   assert!(second_thread.join().unwrap().is_err());
   assert_eq!(second.hub_identity().unwrap(),Some(*boringtun::x25519::PublicKey::from(&boringtun::x25519::StaticSecret::from(key)).as_bytes()));
  }
   #[test] fn references_serialize_with_group_delete_for_all_operations_and_orders(){
    use std::{sync::{mpsc,Arc},thread,time::Duration};
    for (write_op,delete_first) in [("set_acl",true),("set_acl",false),("create_forward",true),("create_forward",false)] {
     let dir=tempfile::tempdir().unwrap();let path=dir.path().join(format!("refs-{write_op}-{delete_first}.db"));
     let seed=Store::open(path.to_str().unwrap()).unwrap();seed.bind_test_identity();seed.setup("10.77.0.0/24","hub.example:51820",25).unwrap();for id in ["victim","source","survivor"]{seed.add_group(&group(id)).unwrap();}
     seed.set_acl("source",&["survivor".into()]).unwrap();let mut peer=peer("peer");peer.group_id="survivor".into();seed.create_peer_allocated(&mut peer).unwrap();drop(seed);
     let winner=Arc::new(Store::open(path.to_str().unwrap()).unwrap());let loser=Arc::new(Store::open(path.to_str().unwrap()).unwrap());
     let (acquired_tx,acquired_rx)=mpsc::channel();let(release_tx,release_rx)=mpsc::channel();let (winner_done_tx,winner_done_rx)=mpsc::channel();let winner_store=winner.clone();let op=write_op.to_string();
     let winner_thread=thread::spawn(move||{TX_TEST_HOOK.with(|h|*h.borrow_mut()=Some((acquired_tx,release_rx)));let result=if delete_first{winner_store.remove_group("victim").map(|_|())}else if op=="set_acl"{winner_store.set_acl("source",&["victim".into(),"survivor".into()]).map(|_|())}else{let mut f=Forward{id:"forward".into(),name:"forward".into(),protocol:"tcp".into(),target_peer_id:"peer".into(),target_port:8100,allowed_group_ids:vec!["victim".into(),"survivor".into()]};winner_store.create_forward(&mut f)};let _=winner_done_tx.send(result);});
     acquired_rx.recv_timeout(Duration::from_secs(3)).expect("winner acquired BEGIN IMMEDIATE");
     let (busy_tx,busy_rx)=mpsc::channel();let(busy_release_tx,busy_release_rx)=mpsc::channel();let(loser_done_tx,loser_done_rx)=mpsc::channel();let loser_store=loser.clone();let loser_op=write_op.to_string();
     let loser_thread=thread::spawn(move||{BUSY_TEST_HOOK.with(|h|*h.borrow_mut()=Some((busy_tx,busy_release_rx)));loser_store.db.lock().unwrap().busy_handler(Some(test_busy_handler)).unwrap();let result=if delete_first{if loser_op=="set_acl"{loser_store.set_acl("source",&["victim".into()]).map(|_|())}else{let mut f=Forward{id:"loser".into(),name:"loser".into(),protocol:"udp".into(),target_peer_id:"peer".into(),target_port:8101,allowed_group_ids:vec!["victim".into()]};loser_store.create_forward(&mut f)}}else{loser_store.remove_group("victim").map(|_|())};let _=loser_done_tx.send(result);});
     busy_rx.recv_timeout(Duration::from_secs(3)).expect("loser reached SQLite busy handler");
     assert!(winner_done_rx.try_recv().is_err());assert!(loser_done_rx.try_recv().is_err());assert!(acquired_rx.try_recv().is_err());
     release_tx.send(()).unwrap();assert!(winner_done_rx.recv_timeout(Duration::from_secs(3)).unwrap().is_ok());
     busy_release_tx.send(()).unwrap();let loser_result=loser_done_rx.recv_timeout(Duration::from_secs(3)).unwrap();winner_thread.join().unwrap();loser_thread.join().unwrap();
     let db=Connection::open(&path).unwrap();let victim:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM groups WHERE id='victim')",[],|r|r.get(0)).unwrap();let source:String=db.query_row("SELECT allowed FROM groups WHERE id='source'",[],|r|r.get(0)).unwrap();let refs:Vec<String>=serde_json::from_str(&source).unwrap();let forwards:i64=db.query_row("SELECT COUNT(*) FROM forwards",[],|r|r.get(0)).unwrap();
     if delete_first {assert!(!victim);assert!(loser_result.unwrap_err().to_string().contains("invalid group reference"));assert_eq!(refs,vec!["survivor"]);assert_eq!(forwards,0);}else{assert!(loser_result.is_ok());assert!(!victim);assert_eq!(refs,vec!["survivor"]);assert_eq!(forwards,if write_op=="create_forward"{1}else{0});if write_op=="create_forward"{let stored:String=db.query_row("SELECT allowed FROM forwards",[],|r|r.get(0)).unwrap();assert_eq!(serde_json::from_str::<Vec<String>>(&stored).unwrap(),vec!["survivor"]);}}
     drop(db);drop(winner);drop(loser);let reopened=Store::open(path.to_str().unwrap()).unwrap();assert!(reopened.groups().unwrap().iter().all(|g|g.id!="victim"));assert!(reopened.group("source").unwrap().is_some());
    }
   }
 #[test] fn open_serializes_behind_immediate_writer(){let d=tempfile::tempdir().unwrap();let p=d.path().join("locked");let path=p.clone();let(ready,wait)=std::sync::mpsc::channel();let(release_tx,release_rx)=std::sync::mpsc::channel();let writer=std::thread::spawn(move||{let db=Connection::open(path).unwrap();let tx=db.unchecked_transaction().unwrap();tx.execute_batch("CREATE TABLE sentinel(value INTEGER)").unwrap();ready.send(()).unwrap();release_rx.recv().unwrap();tx.commit().unwrap();});wait.recv().unwrap();let releaser=std::thread::spawn(move||{std::thread::sleep(std::time::Duration::from_millis(100));release_tx.send(()).unwrap();});assert!(Store::open(p.to_str().unwrap()).is_err());writer.join().unwrap();releaser.join().unwrap();let db=Connection::open(&p).unwrap();let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sentinel')",[],|r|r.get(0)).unwrap();assert!(exists);assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),0);}

#[test]
fn canonical_v3_migrates_without_changing_existing_configuration_or_identity() {
    let dir=tempfile::tempdir().unwrap();let path=dir.path().join("v3.db");
    let s=Store::open(path.to_str().unwrap()).unwrap();s.bind_test_identity();
    s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();s.add_group(&group("group")).unwrap();
    let mut p=peer("existing");s.create_peer_allocated(&mut p).unwrap();let identity=s.hub_identity().unwrap();drop(s);
    let db=Connection::open(&path).unwrap();db.execute_batch("DROP TABLE pending_provisions; PRAGMA user_version=3;").unwrap();drop(db);
    let s=Store::open(path.to_str().unwrap()).unwrap();assert_eq!(s.hub_identity().unwrap(),identity);
    assert_eq!(s.peers().unwrap()[0].id,"existing");assert_eq!(s.peers().unwrap()[0].ipv4,"10.77.0.2");
    assert_eq!(s.network_settings().unwrap().unwrap().endpoint,"hub.example:51820");
    assert_eq!(s.db.lock().unwrap().pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),4);
    assert_eq!(s.recover_pending_provisions().unwrap(),0);
}

#[test]
fn drifted_v3_is_rejected_without_migration() {
    let dir=tempfile::tempdir().unwrap();let path=dir.path().join("drift.db");drop(Store::open(path.to_str().unwrap()).unwrap());
    let db=Connection::open(&path).unwrap();db.execute_batch("DROP TABLE pending_provisions; CREATE INDEX drift ON peers(group_id); PRAGMA user_version=3;").unwrap();drop(db);
    assert!(Store::open(path.to_str().unwrap()).is_err());
    let db=Connection::open(&path).unwrap();assert_eq!(db.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),3);
    let count:i64=db.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE name='pending_provisions'",[],|r|r.get(0)).unwrap();assert_eq!(count,0);
}

#[test]
fn unfinished_provisions_are_hidden_unreferencable_and_recovered_after_restart() {
    let dir=tempfile::tempdir().unwrap();let path=dir.path().join("recovery.db");
    let s=Store::open(path.to_str().unwrap()).unwrap();s.bind_test_identity();s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();s.add_group(&group("group")).unwrap();
    let mut completed=peer("complete");s.begin_peer_provision(&mut completed).unwrap();s.finish_peer_provision(&completed.id).unwrap();
    let mut pending=peer("pending");s.begin_peer_provision(&mut pending).unwrap();
    assert_eq!(s.peers().unwrap().len(),1);assert_eq!(s.runtime_snapshot().unwrap().peers.len(),2);
    let mut f=forward("pending-target");f.target_peer_id=pending.id.clone();assert!(s.create_forward(&mut f).is_err());
    drop(s);
    let s=Store::open(path.to_str().unwrap()).unwrap();assert_eq!(s.recover_pending_provisions().unwrap(),1);
    assert_eq!(s.peers().unwrap()[0].id,"complete");assert_eq!(s.runtime_snapshot().unwrap().peers.len(),1);
    assert_eq!(s.recover_pending_provisions().unwrap(),0);
    let mut retry=peer("pending");s.begin_peer_provision(&mut retry).unwrap();assert_eq!(retry.ipv4,pending.ipv4);
    s.finish_peer_provision(&retry.id).unwrap();assert_eq!(s.peers().unwrap().len(),2);
}

#[test]
fn service_lock_serializes_canonical_paths_and_lives_through_last_arc() {
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service.db");
    let alias_dir = dir.path().join("alias");
    std::os::unix::fs::symlink(dir.path(), &alias_dir).unwrap();
    let store = Arc::new(Store::open_service(&path).unwrap());
    let alias = alias_dir.join("service.db");
    let err = Store::open_service(&alias).err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    let other = Store::open_service(&dir.path().join("other.db")).unwrap();
    drop(other);
    let retained = store.clone();
    drop(store);
    assert_eq!(Store::open_service(&path).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
    drop(retained);
    drop(Store::open_service(&path).unwrap());
}

#[test]
fn service_open_rejects_unsupported_paths_links_and_lock_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    for path in [":memory:", "file:db?mode=memory&cache=shared", ""] {
        assert!(Store::open_service(std::path::Path::new(path)).is_err());
    }
    let target = dir.path().join("target.db");
    drop(Store::open_service(&target).unwrap());
    let hardlink = dir.path().join("hardlink.db");
    std::fs::hard_link(&target, &hardlink).unwrap();
    assert!(Store::open_service(&hardlink).is_err());
    let dangling = dir.path().join("dangling.db");
    std::os::unix::fs::symlink(dir.path().join("missing"), &dangling).unwrap();
    assert!(Store::open_service(&dangling).is_err());
    let symlink_db = dir.path().join("linked.db");
    let db_target = dir.path().join("real.db");
    drop(Store::open_service(&db_target).unwrap());
    std::os::unix::fs::symlink(&db_target, &symlink_db).unwrap();
    // Existing database aliases resolve to the same canonical lock path.
    assert!(Store::open_service(&symlink_db).is_ok());

    let lock_db = dir.path().join("locked.db");
    let lock = dir.path().join("locked.db.wirehub.lock");
    std::os::unix::fs::symlink(&target, &lock).unwrap();
    assert!(Store::open_service(&lock_db).is_err());
}

#[test]
fn fifo_sidecar_is_rejected_without_creating_database() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("fifo.db");
    let lock = dir.path().join("fifo.db.wirehub.lock");
    let status = std::process::Command::new("mkfifo").arg(&lock).status().unwrap();
    assert!(status.success());
    assert!(Store::open_service(&database).is_err());
    assert!(!database.exists());
}

#[test]
fn service_lock_is_acquired_before_v3_migration() {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3-locked.db");
    let seed = Store::open(path.to_str().unwrap()).unwrap();
    drop(seed);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE pending_provisions; PRAGMA user_version=3;").unwrap();
    drop(db);
    let lock_path = dir.path().join("v3-locked.db.wirehub.lock");
    let holder = std::fs::OpenOptions::new().read(true).write(true).create(true).mode(0o600).open(&lock_path).unwrap();
    holder.try_lock().unwrap();
    assert_eq!(Store::open_service(&path).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
    let db = Connection::open(&path).unwrap();
    assert_eq!(db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE name='pending_provisions'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn schema_failure_releases_the_instance_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid-schema.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE old(value TEXT); PRAGMA user_version=1;").unwrap();
    drop(db);
    assert!(Store::open_service(&path).is_err());
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join("invalid-schema.db.wirehub.lock"))
        .unwrap();
    lock.try_lock().unwrap();
}

#[test]
fn competing_service_cannot_recover_active_pending_peer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending-service.db");
    let store = Store::open_service(&path).unwrap();
    store.bind_test_identity();
    store.setup("10.77.0.0/24", "hub.example:51820", 25).unwrap();
    store.add_group(&group("group")).unwrap();
    let mut pending = peer("in-flight");
    store.begin_peer_provision(&mut pending).unwrap();

    assert_eq!(Store::open_service(&path).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
    let db = Connection::open(&path).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM pending_provisions WHERE peer_id='in-flight'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM peers WHERE id='in-flight'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    drop(db);

    assert_eq!(store.finish_peer_provision("in-flight").unwrap(), 1);
    drop(store);
    let restarted = Store::open_service(&path).unwrap();
    assert_eq!(restarted.recover_pending_provisions().unwrap(), 0);
    let mut interrupted = peer("interrupted");
    restarted.begin_peer_provision(&mut interrupted).unwrap();
    drop(restarted);
    let restarted = Store::open_service(&path).unwrap();
    assert_eq!(restarted.recover_pending_provisions().unwrap(), 1);
    assert!(restarted.peers().unwrap().iter().all(|peer| peer.id != "interrupted"));
}

#[test]
fn service_lock_child_helper() {
    use std::io::{BufRead, Write};

    let Ok(path) = std::env::var("WIREHUB_LOCK_CHILD_DB") else {
        return;
    };
    println!("READY");
    std::io::stdout().flush().unwrap();
    let mut command = String::new();
    std::io::stdin().lock().read_line(&mut command).unwrap();
    assert_eq!(command.trim(), "GO");

    match Store::open_service(std::path::Path::new(&path)) {
        Ok(_store) => {
            println!("LOCK_HELD");
            std::io::stdout().flush().unwrap();
            let mut command = String::new();
            std::io::stdin().lock().read_line(&mut command).unwrap();
            assert_eq!(command.trim(), "RELEASE");
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => println!("LOCK_BUSY"),
        Err(error) => panic!("unexpected child lock error: {error}"),
    }
}

struct LockChild(std::process::Child);

impl Drop for LockChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn spawn_lock_child(path: &std::path::Path) -> (LockChild, std::sync::mpsc::Receiver<String>) {
    use std::{io::{BufRead, BufReader}, process::{Command, Stdio}, sync::mpsc};

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "storage::tests::service_lock_child_helper", "--nocapture"])
        .env("WIREHUB_LOCK_CHILD_DB", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() { break; }
        }
    });
    (LockChild(child), rx)
}

fn wait_lock_child(child: &mut LockChild) -> std::process::ExitStatus {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() { return status; }
        if Instant::now() >= deadline {
            let _ = child.0.kill();
            let _ = child.0.wait();
            panic!("child process exit timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn child_line(rx: &std::sync::mpsc::Receiver<String>, wanted: &str) {
    use std::time::Duration;
    loop {
        let line = rx.recv_timeout(Duration::from_secs(5)).expect("child handshake timed out");
        if line.trim() == wanted { return; }
    }
}

fn child_command(child: &mut LockChild, command: &str) {
    use std::io::Write;
    writeln!(child.0.stdin.as_mut().unwrap(), "{command}").unwrap();
    child.0.stdin.as_mut().unwrap().flush().unwrap();
}

#[test]
fn active_pending_peer_survives_cross_process_symlink_contender() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("active.db");
    let symlink = dir.path().join("active-alias.db");
    let store = Store::open_service(&path).unwrap();
    store.bind_test_identity();
    store.setup("10.77.0.0/24", "hub.example:51820", 25).unwrap();
    store.add_group(&group("group")).unwrap();
    let mut pending = peer("in-flight");
    store.begin_peer_provision(&mut pending).unwrap();
    std::os::unix::fs::symlink(&path, &symlink).unwrap();

    let (mut child, rx) = spawn_lock_child(&symlink);
    child_line(&rx, "READY");
    child_command(&mut child, "GO");
    child_line(&rx, "LOCK_BUSY");
    assert!(wait_lock_child(&mut child).success());

    let db = Connection::open(&path).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM pending_provisions WHERE peer_id='in-flight'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM peers WHERE id='in-flight'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    drop(db);
    assert_eq!(store.finish_peer_provision("in-flight").unwrap(), 1);
}

#[test]
fn simultaneous_process_open_has_one_holder_and_reopens_after_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("race.db");
    let (mut first, first_rx) = spawn_lock_child(&path);
    let (mut second, second_rx) = spawn_lock_child(&path);
    child_line(&first_rx, "READY");
    child_line(&second_rx, "READY");
    child_command(&mut first, "GO");
    child_command(&mut second, "GO");
    let first_result = first_rx.recv_timeout(std::time::Duration::from_secs(5)).expect("first contender timed out");
    let second_result = second_rx.recv_timeout(std::time::Duration::from_secs(5)).expect("second contender timed out");
    let (holder, loser) = match (first_result.trim(), second_result.trim()) {
        ("LOCK_HELD", "LOCK_BUSY") => (&mut first, &mut second),
        ("LOCK_BUSY", "LOCK_HELD") => (&mut second, &mut first),
        other => panic!("unexpected contender results: {other:?}"),
    };
    assert!(wait_lock_child(loser).success());
    child_command(holder, "RELEASE");
    assert!(wait_lock_child(holder).success());

    let store = Store::open_service(&path).unwrap();
    assert_eq!(store.network_settings().unwrap(), None);
}

#[test]
fn process_exit_releases_service_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("child-held.db");
    let (mut child, rx) = spawn_lock_child(&path);
    child_line(&rx, "READY");
    child_command(&mut child, "GO");
    child_line(&rx, "LOCK_HELD");
    child.0.kill().unwrap();
    let _ = wait_lock_child(&mut child);
    drop(child);
    drop(Store::open_service(&path).unwrap());
}
