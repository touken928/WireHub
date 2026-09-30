use std::sync::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use crate::{model::{Forward, Group, Peer}, network::{self, NetworkSettings, Subnet24}};

const SCHEMA_VERSION: i64 = 2;
const TABLES: &str = "
CREATE TABLE groups(id TEXT PRIMARY KEY,name TEXT NOT NULL,allowed TEXT NOT NULL DEFAULT '[]');
CREATE TABLE peers(id TEXT PRIMARY KEY,name TEXT NOT NULL,public_key TEXT NOT NULL UNIQUE,ipv4 TEXT NOT NULL UNIQUE,group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE RESTRICT,rx INTEGER NOT NULL DEFAULT 0,tx INTEGER NOT NULL DEFAULT 0,last_handshake INTEGER);
CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL,UNIQUE(protocol,target_port));
CREATE TABLE network_settings(id INTEGER PRIMARY KEY CHECK(id=1),subnet TEXT NOT NULL,endpoint TEXT NOT NULL,persistent_keepalive INTEGER NOT NULL CHECK(persistent_keepalive BETWEEN 0 AND 65535));
CREATE UNIQUE INDEX groups_name_unique ON groups(name);
CREATE UNIQUE INDEX peers_name_unique ON peers(name);
";

pub struct Store { db: Mutex<Connection> }

/// A coherent view of the configuration used to initialize a runtime.
pub struct RuntimeSnapshot {
    pub settings: Option<NetworkSettings>,
    pub groups: Vec<Group>,
    pub peers: Vec<Peer>,
    pub forwards: Vec<Forward>,
}
impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let mut db=Connection::open(path)?;
        db.pragma_update(None,"foreign_keys","ON")?;
        let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version:i64=tx.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if version > SCHEMA_VERSION { return Err(sql_error("database schema version is newer than this WireHub version; upgrade the application")); }
        if version < 0 || version != 0 && version != SCHEMA_VERSION { return Err(sql_error("database schema version is unsupported (including v1); export any needed data and create a fresh database; refusing to modify database")); }
        if version == 0 {
            let has_application_objects:bool={
                let mut stmt=tx.prepare("SELECT name FROM sqlite_master WHERE type IN ('table','index','view','trigger')")?;
                let names=stmt.query_map([],|r|r.get::<_,String>(0))?;
                let mut found=false;
                for name in names { if !name?.starts_with("sqlite_") { found=true; break; } }
                found
            };
            if has_application_objects {
                return Err(sql_error("unversioned database contains existing schema objects; refusing to modify it"));
            }
            tx.execute_batch(TABLES)?;
            tx.pragma_update(None,"user_version",SCHEMA_VERSION)?;
        } else {
            validate_current_schema(&tx)?;
            validate_current_settings(&tx)?;
        }
        tx.commit()?;
        Ok(Self{db:Mutex::new(db)})
    }
    pub fn setup(&self, subnet:&str, endpoint:&str, keepalive:u32)->rusqlite::Result<NetworkSettings>{
        let settings=network::validate_settings(subnet,endpoint,keepalive).map_err(sql_error)?;
        let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?; let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count:i64=tx.query_row("SELECT COUNT(*) FROM network_settings",[],|r|r.get(0))?;
        if count!=0{return Err(sql_error("network setup has already been completed"));}
        tx.execute("INSERT INTO network_settings(id,subnet,endpoint,persistent_keepalive) VALUES(1,?1,?2,?3)",params![settings.subnet,settings.endpoint,settings.persistent_keepalive])?;
        tx.commit()?; Ok(settings)
    }
    pub fn update_settings(&self, endpoint:&str, keepalive:u32)->rusqlite::Result<NetworkSettings>{
        let current=self.network_settings()?.ok_or_else(||sql_error("network setup is required"))?;
        let settings=network::validate_settings(&current.subnet,endpoint,keepalive).map_err(sql_error)?;
        let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;
        let tx=db.transaction()?;
        let count=tx.execute("UPDATE network_settings SET endpoint=?1,persistent_keepalive=?2 WHERE id=1",params![settings.endpoint,settings.persistent_keepalive])?;
        if count!=1{return Err(sql_error("network setup is required"));}
        tx.commit()?;Ok(settings)
    }
    pub fn network_settings(&self)->rusqlite::Result<Option<NetworkSettings>>{
        let db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;
        read_network_settings(&db)
    }
    pub fn groups(&self)->rusqlite::Result<Vec<Group>> { let db=self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?; read_groups(&db) }
    pub fn runtime_snapshot(&self)->rusqlite::Result<RuntimeSnapshot>{
        let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;
        let tx=db.transaction()?;
        let snapshot=RuntimeSnapshot { settings:read_network_settings(&tx)?, groups:read_groups(&tx)?, peers:read_peers(&tx)?, forwards:read_forwards(&tx)? };
        tx.commit()?;
        Ok(snapshot)
    }
    pub fn group(&self,id:&str)->rusqlite::Result<Option<Group>> { Ok(self.groups()?.into_iter().find(|x|x.id==id)) }
     pub fn add_group(&self,g:&Group)->rusqlite::Result<()> {let name=normalized_name(&g.name)?;let mut db=self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;ensure_setup(&tx)?;tx.execute("INSERT INTO groups(id,name,allowed) VALUES(?1,?2,'[]')",params![g.id,name])?;tx.commit()?;Ok(())}
    pub fn remove_group(&self,id:&str)->rusqlite::Result<usize>{let mut db=self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;let tx=db.transaction()?;let mut q=tx.prepare("SELECT id,allowed FROM groups WHERE id<>?1")?;let rows=q.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;let mut updates=Vec::new();for row in rows {let (group_id,raw)=row?;let mut acl:Vec<String>=serde_json::from_str(&raw).map_err(|_|rusqlite::Error::InvalidQuery)?;if acl.iter().any(|x|x==id){acl.retain(|x|x!=id);updates.push((group_id,serde_json::to_string(&acl).map_err(|_|rusqlite::Error::InvalidQuery)?));}} drop(q);for (group_id,acl) in updates {tx.execute("UPDATE groups SET allowed=?2 WHERE id=?1",params![group_id,acl])?;}let count=tx.execute("DELETE FROM groups WHERE id=?1",[id])?;tx.commit()?;Ok(count)}
    pub fn set_acl(&self,id:&str,allowed:&[String])->rusqlite::Result<usize>{let encoded=serde_json::to_string(allowed).map_err(|_|rusqlite::Error::InvalidQuery)?;Ok(self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?.execute("UPDATE groups SET allowed=?2 WHERE id=?1",params![id,encoded])?)}
    pub fn peers(&self)->rusqlite::Result<Vec<Peer>> {let db=self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;read_peers(&db)}
    #[cfg(test)]
    pub fn add_peer(&self,p:&Peer)->rusqlite::Result<()> {let name=normalized_name(&p.name)?;self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?.execute("INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES(?1,?2,?3,?4,?5)",params![p.id,name,p.public_key,p.ipv4,p.group_id])?;Ok(())}
      pub fn create_peer_allocated(&self,p:&mut Peer)->rusqlite::Result<bool>{let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;let subnet=settings_subnet(&tx)?;let name=normalized_name(&p.name)?;let mut candidate=None;for i in 2..=254{let ip=subnet.peer_ip(i).unwrap();let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM peers WHERE ipv4=?1)",[&ip],|r|r.get(0))?;if !used{candidate=Some(ip);break}}let Some(ip)=candidate else{return Ok(false)};p.ipv4=ip;tx.execute("INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES(?1,?2,?3,?4,?5)",params![p.id,name,p.public_key,p.ipv4,p.group_id])?;tx.commit()?;p.name=name;Ok(true)}
    pub fn move_peer(&self,id:&str,g:&str)->rusqlite::Result<usize>{Ok(self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?.execute("UPDATE peers SET group_id=?2 WHERE id=?1",params![id,g])?)}
    pub fn remove_peer(&self,id:&str)->rusqlite::Result<usize>{Ok(self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?.execute("DELETE FROM peers WHERE id=?1",[id])?)}
     pub fn forwards(&self)->rusqlite::Result<Vec<Forward>> {let db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;read_forwards(&db)}
       pub fn create_forward(&self,f:&mut Forward)->rusqlite::Result<()> {let allowed=serde_json::to_string(&f.allowed_group_ids).map_err(|_|rusqlite::Error::InvalidQuery)?;let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;ensure_setup(&tx)?;let name=normalized_name(&f.name)?;insert_forward(&tx,f,&name,&allowed)?;tx.commit()?;f.name=name;Ok(())}
    pub fn remove_forward(&self,id:&str)->rusqlite::Result<usize>{Ok(self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?.execute("DELETE FROM forwards WHERE id=?1",[id])?)}
}

fn read_network_settings(db:&Connection)->rusqlite::Result<Option<NetworkSettings>>{
    db.query_row("SELECT subnet,endpoint,persistent_keepalive FROM network_settings WHERE id=1",[],|r|Ok(NetworkSettings{subnet:r.get(0)?,endpoint:r.get(1)?,persistent_keepalive:r.get::<_,u16>(2)?})).optional()
}
fn read_groups(db:&Connection)->rusqlite::Result<Vec<Group>>{
    let mut q=db.prepare("SELECT id,name,allowed FROM groups")?;
    let rows=q.query_map([],|r| { let a:String=r.get(2)?; let allowed=serde_json::from_str(&a).map_err(|_|rusqlite::Error::InvalidQuery)?; Ok(Group{id:r.get(0)?,name:r.get(1)?,allowed_groups:allowed}) })?;
    rows.collect()
}
fn read_peers(db:&Connection)->rusqlite::Result<Vec<Peer>>{
    let mut q=db.prepare("SELECT id,trim(name),public_key,ipv4,group_id,rx,tx,last_handshake FROM peers")?;
    let rows=q.query_map([],|r|Ok(Peer{id:r.get(0)?,name:r.get(1)?,public_key:r.get(2)?,ipv4:r.get(3)?,group_id:r.get(4)?,received_bytes:r.get(5)?,sent_bytes:r.get(6)?,last_handshake_unix:r.get(7)?}))?;
    rows.collect()
}
fn read_forwards(db:&Connection)->rusqlite::Result<Vec<Forward>>{
    let mut q=db.prepare("SELECT id,trim(name),protocol,target_peer_id,target_port,allowed FROM forwards ORDER BY id")?;
    let rows=q.query_map([],|r|{let raw:String=r.get(5)?;Ok(Forward{id:r.get(0)?,name:r.get(1)?,protocol:r.get(2)?,target_peer_id:r.get(3)?,target_port:r.get(4)?,allowed_group_ids:serde_json::from_str(&raw).map_err(|_|rusqlite::Error::InvalidQuery)?})})?;
    rows.collect()
}

fn insert_forward(tx:&Transaction<'_>,f:&Forward,name:&str,allowed:&str)->rusqlite::Result<()> {tx.execute("INSERT INTO forwards(id,name,protocol,target_peer_id,target_port,allowed) VALUES(?1,?2,?3,?4,?5,?6)",params![f.id,name,f.protocol,f.target_peer_id,f.target_port,allowed])?;Ok(())}
fn settings_subnet(tx:&Transaction<'_>)->rusqlite::Result<Subnet24>{let raw:Option<String>=tx.query_row("SELECT subnet FROM network_settings WHERE id=1",[],|r|r.get(0)).optional()?;Subnet24::parse(&raw.ok_or_else(||sql_error("network setup is required before allocating addresses"))?).map_err(sql_error)}
fn ensure_setup(tx:&Transaction<'_>)->rusqlite::Result<()> { let _:String=tx.query_row("SELECT subnet FROM network_settings WHERE id=1",[],|r|r.get(0)).optional()?.ok_or_else(||sql_error("network setup is required"))?;Ok(()) }
fn normalized_name(name:&str)->rusqlite::Result<String>{let value=name.trim();if value.is_empty(){return Err(sql_error("name must not be empty"));}Ok(value.into())}
fn sql_error(message:impl Into<String>)->rusqlite::Error{rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput,message.into())))}

fn table_exists(db:&Connection, table:&str)->rusqlite::Result<bool>{
    db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",[table],|r|r.get(0))
}

fn validate_current_schema(db:&Connection)->rusqlite::Result<()> {
    for table in ["groups", "peers", "forwards", "network_settings"] {
        if !table_exists(db, table)? { return Err(sql_error("current database schema is incomplete; refusing to modify database")); }
    }
    let mut stmt=db.prepare("PRAGMA foreign_key_list(peers)")?;
    let peer_fk=stmt.query_map([],|r|Ok((r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(6)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !peer_fk.iter().any(|(table,from,to,on_delete)|table=="groups"&&from=="group_id"&&to=="id"&&on_delete.eq_ignore_ascii_case("RESTRICT")) {
        return Err(sql_error("current database schema is missing the peers.group_id foreign key; refusing to modify database"));
    }
    let mut stmt=db.prepare("PRAGMA table_info(forwards)")?;
    let columns=stmt.query_map([],|r|Ok((r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(5)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if columns != [("id".into(),"TEXT".into(),0,1),("name".into(),"TEXT".into(),1,0),("protocol".into(),"TEXT".into(),1,0),("target_peer_id".into(),"TEXT".into(),1,0),("target_port".into(),"INTEGER".into(),1,0),("allowed".into(),"TEXT".into(),1,0)] {
        return Err(sql_error("current forwards table has an invalid column shape; refusing to modify database"));
    }
    let mut stmt=db.prepare("PRAGMA index_list(forwards)")?;
    let indexes=stmt.query_map([],|r|Ok((r.get::<_,String>(1)?,r.get::<_,i64>(2)?!=0,r.get::<_,i64>(4)?!=0)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut correct=false;
    for (name,unique,partial) in indexes { if !unique || partial {continue} if index_key_columns(db,&name)?==[ (Some("protocol".to_string()),Some("BINARY".to_string())), (Some("target_port".to_string()),Some("BINARY".to_string())) ] {correct=true;break} }
    if !correct { return Err(sql_error("current database schema is missing the forwards uniqueness constraint; refusing to modify database")); }
    let invalid:i64=db.query_row("SELECT COUNT(*) FROM forwards WHERE protocol NOT IN ('tcp','udp') OR target_port NOT BETWEEN 1 AND 65535",[],|r|r.get(0))?;
    if invalid != 0 { return Err(sql_error("current database contains an invalid forward protocol or port; refusing to modify database")); }
    for (table,expected) in [("groups",vec!["name"]),("peers",vec!["name"])] {
        let mut stmt=db.prepare(&format!("PRAGMA index_list({table})"))?;
        let indexes=stmt.query_map([],|r|Ok((r.get::<_,String>(1)?,r.get::<_,i64>(2)?!=0,r.get::<_,i64>(4)?!=0)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut found=false;
        for (name,unique,partial) in indexes {
            if !unique || partial {continue}
            let columns=index_key_columns(db,&name)?;
            if columns.len()==expected.len()
                && columns.iter().zip(&expected).all(|((column,collation),expected)|
                    column.as_deref()==Some(*expected) && collation.as_deref().is_some_and(|value|value.eq_ignore_ascii_case("BINARY"))) {found=true;break}
        }
        if !found {return Err(sql_error("current database schema is missing a unique name constraint; refusing to modify database"));}
    }
    Ok(())
}

fn index_key_columns(db:&Connection,name:&str)->rusqlite::Result<Vec<(Option<String>,Option<String>)>> {
    let escaped=name.replace('\'', "''");
    let mut info=db.prepare(&format!("PRAGMA index_xinfo('{}')",escaped))?;
    let columns=info.query_map([],|r|Ok((r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(4)?,r.get::<_,i64>(5)?!=0)))?
        .filter_map(|row|match row { Ok((column,collation,true))=>Some(Ok((column,collation))),Ok(_)=>None,Err(error)=>Some(Err(error)) })
        .collect();
    columns
}

fn validate_current_settings(db:&Connection)->rusqlite::Result<()> {
    let settings:Option<(String,String,u32)>=db.query_row("SELECT subnet,endpoint,persistent_keepalive FROM network_settings WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((subnet,endpoint,keepalive))=settings {
        network::validate_settings(&subnet,&endpoint,keepalive).map_err(|_|sql_error("current database contains invalid network settings; refusing to modify database"))?;
        let count:i64=db.query_row("SELECT COUNT(*) FROM network_settings",[],|r|r.get(0))?;
        if count!=1 {return Err(sql_error("current database contains invalid network settings; refusing to modify database"));}
    } else {
        let count:i64=db.query_row("SELECT COUNT(*) FROM network_settings",[],|r|r.get(0))?;
        if count!=0 {return Err(sql_error("current database contains invalid network settings; refusing to modify database"));}
    }
    Ok(())
}

#[cfg(test)] mod tests {
 use super::*; use rusqlite::Connection;
 fn group(id:&str)->Group{Group{id:id.into(),name:id.into(),allowed_groups:vec![]}}
 fn peer(id:&str)->Peer{Peer{id:id.into(),name:id.into(),public_key:format!("key-{id}"),ipv4:String::new(),group_id:"group".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}}
 fn forward(id:&str)->Forward{Forward{id:id.into(),name:id.into(),protocol:"tcp".into(),target_peer_id:"peer".into(),target_port:9000,allowed_group_ids:vec!["group".into()]}}
 fn store()->Store{let s=Store::open(":memory:").unwrap();s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();s}
   #[test] fn setup_once_persists_and_allocations_are_disjoint_transactional(){let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db");let s=Store::open(path.to_str().unwrap()).unwrap();assert!(s.create_peer_allocated(&mut peer("no-settings")).is_err());assert!(s.add_group(&group("no-setup")).is_err());assert_eq!(s.network_settings().unwrap(),None);let settings=s.setup("10.77.0.0/24","hub.example:51820",25).unwrap();assert_eq!(settings.subnet,"10.77.0.0/24");assert!(s.setup("10.77.0.0/24","hub.example:51820",25).is_err());drop(s);let s=Store::open(path.to_str().unwrap()).unwrap();assert_eq!(s.network_settings().unwrap().unwrap().endpoint,"hub.example:51820");s.add_group(&group("group")).unwrap();let mut p=peer("peer");assert!(s.create_peer_allocated(&mut p).unwrap());assert_eq!(p.ipv4,"10.77.0.2");let mut f=forward("f");s.create_forward(&mut f).unwrap();assert_eq!(s.forwards().unwrap().len(),1);}
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
  #[test] fn fresh_database_initializes_current_schema(){let d=tempfile::tempdir().unwrap();let p=d.path().join("fresh");let s=Store::open(p.to_str().unwrap()).unwrap();assert_eq!(s.network_settings().unwrap(),None);drop(s);let db=Connection::open(&p).unwrap();assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),SCHEMA_VERSION);for table in ["groups","peers","forwards","network_settings"]{assert!(table_exists(&db,table).unwrap());}}
  #[test] fn forward_uniqueness_is_protocol_and_target_port_and_has_no_pool_limit(){let s=store();s.add_group(&group("group")).unwrap();for id in ["peer","peer2"]{let mut p=peer(id);s.create_peer_allocated(&mut p).unwrap();}let mut tcp=forward("one");tcp.target_peer_id="peer".into();tcp.target_port=443;s.create_forward(&mut tcp).unwrap();let mut udp=Forward{protocol:"udp".into(),..forward("udp")};udp.target_port=443;s.create_forward(&mut udp).unwrap();let mut dup=Forward{id:"dup".into(),target_peer_id:"peer2".into(),allowed_group_ids:vec!["other".into()],..forward("dup")};dup.target_port=443;assert!(s.create_forward(&mut dup).is_err());for port in 1..=140 {let mut f=Forward{id:format!("f{port}"),protocol:"tcp".into(),target_peer_id:"peer".into(),target_port:1000+port, ..forward("many")};s.create_forward(&mut f).unwrap();}assert_eq!(s.forwards().unwrap().len(),142);}
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
 #[test] fn open_serializes_behind_immediate_writer(){let d=tempfile::tempdir().unwrap();let p=d.path().join("locked");let path=p.clone();let(ready,wait)=std::sync::mpsc::channel();let(release_tx,release_rx)=std::sync::mpsc::channel();let writer=std::thread::spawn(move||{let db=Connection::open(path).unwrap();let tx=db.unchecked_transaction().unwrap();tx.execute_batch("CREATE TABLE sentinel(value INTEGER)").unwrap();ready.send(()).unwrap();release_rx.recv().unwrap();tx.commit().unwrap();});wait.recv().unwrap();let releaser=std::thread::spawn(move||{std::thread::sleep(std::time::Duration::from_millis(100));release_tx.send(()).unwrap();});assert!(Store::open(p.to_str().unwrap()).is_err());writer.join().unwrap();releaser.join().unwrap();let db=Connection::open(&p).unwrap();assert!(table_exists(&db,"sentinel").unwrap());assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),0);}
}
