use std::sync::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use crate::{model::{Forward, Group, Peer}, network::{self, NetworkSettings, Subnet24}};

const SCHEMA_VERSION: i64 = 3;
const TABLES: &str = "
CREATE TABLE groups(id TEXT PRIMARY KEY,name TEXT NOT NULL,allowed TEXT NOT NULL DEFAULT '[]');
CREATE TABLE peers(id TEXT PRIMARY KEY,name TEXT NOT NULL,public_key TEXT NOT NULL UNIQUE,ipv4 TEXT NOT NULL UNIQUE,group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE RESTRICT,rx INTEGER NOT NULL DEFAULT 0,tx INTEGER NOT NULL DEFAULT 0,last_handshake INTEGER);
CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL,UNIQUE(protocol,target_port));
CREATE TABLE network_settings(id INTEGER PRIMARY KEY CHECK(id=1),subnet TEXT NOT NULL,endpoint TEXT NOT NULL,persistent_keepalive INTEGER NOT NULL CHECK(persistent_keepalive BETWEEN 0 AND 65535));
CREATE TABLE hub_identity(id INTEGER PRIMARY KEY CHECK(id=1),public_key BLOB NOT NULL CHECK(typeof(public_key)='blob' AND length(public_key)=32));
CREATE UNIQUE INDEX groups_name_unique ON groups(name);
CREATE UNIQUE INDEX peers_name_unique ON peers(name);
";

pub struct Store { db: Mutex<Connection> }

#[cfg(test)]
thread_local! { static TX_TEST_HOOK: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>,std::sync::mpsc::Receiver<()>)>>=const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
thread_local! { static BUSY_TEST_HOOK: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>,std::sync::mpsc::Receiver<()>)>>=const { std::cell::RefCell::new(None) }; }
#[cfg(test)] fn after_begin_immediate(){TX_TEST_HOOK.with(|h|if let Some((tx,rx))=h.borrow().as_ref(){let _=tx.send(());let _=rx.recv_timeout(std::time::Duration::from_secs(5));});}
#[cfg(not(test))] fn after_begin_immediate(){}
#[cfg(test)] fn test_busy_handler(_:i32)->bool{BUSY_TEST_HOOK.with(|h|h.borrow().as_ref().map(|(tx,rx)|{let _=tx.send(());rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok()}).unwrap_or(false))}

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
        if version != 0 && version != SCHEMA_VERSION { return Err(sql_error("database schema version is unsupported (including v1/v2); export any needed data and create a fresh database; refusing to modify database")); }
        if version == 0 {
            let has_application_objects:bool={
                let mut stmt=tx.prepare("SELECT name FROM sqlite_schema")?;
                let rows=stmt.query_map([],|r|r.get::<_,String>(0))?;
                rows.count()!=0
            };
            if has_application_objects {
                return Err(sql_error("unversioned database contains existing schema objects; refusing to modify it"));
            }
            tx.execute_batch(TABLES)?;
            tx.pragma_update(None,"user_version",SCHEMA_VERSION)?;
        } else {
            validate_current_schema(&tx)?;
            validate_current_settings(&tx)?;
            validate_current_data(&tx)?;
        }
        tx.commit()?;
        Ok(Self{db:Mutex::new(db)})
    }
    pub fn setup(&self, subnet:&str, endpoint:&str, keepalive:u32)->rusqlite::Result<NetworkSettings>{
        let settings=network::validate_settings(subnet,endpoint,keepalive).map_err(sql_error)?;
        let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?; let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let identity:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM hub_identity WHERE id=1)",[],|r|r.get(0))?;
        if !identity{return Err(sql_error("hub identity is required before network setup"));}
        let count:i64=tx.query_row("SELECT COUNT(*) FROM network_settings",[],|r|r.get(0))?;
        if count!=0{return Err(sql_error("network setup has already been completed"));}
        tx.execute("INSERT INTO network_settings(id,subnet,endpoint,persistent_keepalive) VALUES(1,?1,?2,?3)",params![settings.subnet,settings.endpoint,settings.persistent_keepalive])?;
        tx.commit()?; Ok(settings)
    }
    #[cfg(test)]
    pub fn hub_identity(&self)->rusqlite::Result<Option<[u8;32]>> {
        let db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;
        let configured:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)",[],|r|r.get(0))?;
        let key:Option<Vec<u8>>=db.query_row("SELECT public_key FROM hub_identity WHERE id=1",[],|r|r.get(0)).optional()?;
        if configured && key.is_none(){return Err(sql_error("configured database is missing its hub identity"));}
        key.map(|bytes|bytes.try_into().map_err(|_|sql_error("invalid hub identity"))).transpose()
    }
    #[cfg(test)]
    pub fn bind_hub_identity(&self,key:&[u8;32])->rusqlite::Result<()> {
        let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let configured:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)",[],|r|r.get(0))?;
        let existing:Option<Vec<u8>>=tx.query_row("SELECT public_key FROM hub_identity WHERE id=1",[],|r|r.get(0)).optional()?;
        if let Some(existing)=existing {if existing.as_slice()!=key{return Err(sql_error("hub key does not match persisted hub identity"));}}
        else {if configured{return Err(sql_error("configured database is missing its hub identity"));}tx.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,?1)",[key.as_slice()])?;}
        tx.commit()?;Ok(())
    }
    pub fn bootstrap_hub_identity<F,L,P>(&self,unbound:F,bound:L,publish:P)->std::io::Result<[u8;32]>
    where F:FnOnce()->std::io::Result<[u8;32]>, L:FnOnce(Option<[u8;32]>)->std::io::Result<[u8;32]>, P:FnOnce(&[u8;32])->std::io::Result<[u8;32]> {
        let mut db=self.db.lock().map_err(|_|std::io::Error::other("database lock poisoned"))?;
        let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(io_db)?;
        let configured:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)",[],|r|r.get(0)).map_err(io_db)?;
        let rows:i64=tx.query_row("SELECT COUNT(*) FROM hub_identity",[],|r|r.get(0)).map_err(io_db)?;
        let expected:Option<Vec<u8>>=tx.query_row("SELECT public_key FROM hub_identity WHERE id=1",[],|r|r.get(0)).optional().map_err(io_db)?;
        if rows>1 || (rows==1 && expected.is_none()){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"invalid persisted hub identity"));}
        let expected=expected.map(|v|v.try_into().map_err(|_|std::io::Error::new(std::io::ErrorKind::InvalidData,"invalid persisted hub identity"))).transpose()?;
        let key=if expected.is_some(){bound(expected)?}else{
            let inventory:i64=tx.query_row("SELECT (SELECT COUNT(*) FROM groups)+(SELECT COUNT(*) FROM peers)+(SELECT COUNT(*) FROM forwards)+(SELECT COUNT(*) FROM network_settings)",[],|r|r.get(0)).map_err(io_db)?;
            if configured || inventory!=0{return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"unbound database contains configuration"));}
            let key=unbound()?;publish(&key)?
        };
        let public=*boringtun::x25519::PublicKey::from(&boringtun::x25519::StaticSecret::from(key)).as_bytes();
        if let Some(expected)=expected {if expected!=public{return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key does not match persisted hub identity"));}}
        else {tx.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,?1)",[public.as_slice()]).map_err(io_db)?;}
        tx.commit().map_err(io_db)?;Ok(key)
    }
    #[cfg(test)] pub fn bind_test_identity(&self){self.bind_hub_identity(&[7u8;32]).unwrap();}
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
      pub fn remove_group(&self,id:&str)->rusqlite::Result<usize>{let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;after_begin_immediate();let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",[id],|r|r.get(0))?;if !exists{return Ok(0)}let mut updates=Vec::new();for (table,column) in [("groups","allowed"),("forwards","allowed")] {let mut q=tx.prepare(&format!("SELECT id,{column} FROM {table}"))?;let rows=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;for row in rows{let(rid,raw)=row?;let mut refs:Vec<String>=serde_json::from_str(&raw).map_err(|_|sql_error("malformed group references"))?;if refs.iter().any(|x|x==id){refs.retain(|x|x!=id);updates.push((table.to_owned(),rid,serde_json::to_string(&refs).map_err(|_|rusqlite::Error::InvalidQuery)?));}}}for(table,rid,refs)in updates{tx.execute(&format!("UPDATE {table} SET allowed=?2 WHERE id=?1"),params![rid,refs])?;}let n=tx.execute("DELETE FROM groups WHERE id=?1",[id])?;tx.commit()?;Ok(n)}
      pub fn set_acl(&self,id:&str,allowed:&[String])->rusqlite::Result<usize>{let encoded=serde_json::to_string(allowed).map_err(|_|rusqlite::Error::InvalidQuery)?;let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;after_begin_immediate();let source:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",[id],|r|r.get(0))?;if !source{return Ok(0)}validate_group_refs(&tx,allowed)?;let n=tx.execute("UPDATE groups SET allowed=?2 WHERE id=?1",params![id,encoded])?;tx.commit()?;Ok(n)}
    pub fn peers(&self)->rusqlite::Result<Vec<Peer>> {let db=self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;read_peers(&db)}
    #[cfg(test)]
    pub fn add_peer(&self,p:&Peer)->rusqlite::Result<()> {let name=normalized_name(&p.name)?;self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?.execute("INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES(?1,?2,?3,?4,?5)",params![p.id,name,p.public_key,p.ipv4,p.group_id])?;Ok(())}
      pub fn create_peer_allocated(&self,p:&mut Peer)->rusqlite::Result<bool>{let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;let subnet=settings_subnet(&tx)?;validate_group_refs(&tx,&[p.group_id.clone()])?;let name=normalized_name(&p.name)?;let mut candidate=None;for i in 2..=254{let ip=subnet.peer_ip(i).unwrap();let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM peers WHERE ipv4=?1)",[&ip],|r|r.get(0))?;if !used{candidate=Some(ip);break}}let Some(ip)=candidate else{return Ok(false)};p.ipv4=ip;tx.execute("INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES(?1,?2,?3,?4,?5)",params![p.id,name,p.public_key,p.ipv4,p.group_id])?;tx.commit()?;p.name=name;Ok(true)}
     pub fn move_peer(&self,id:&str,g:&str)->rusqlite::Result<usize>{let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;validate_group_refs(&tx,&[g.to_string()])?;let n=tx.execute("UPDATE peers SET group_id=?2 WHERE id=?1",params![id,g])?;tx.commit()?;Ok(n)}
    pub fn remove_peer(&self,id:&str)->rusqlite::Result<usize>{Ok(self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?.execute("DELETE FROM peers WHERE id=?1",[id])?)}
     pub fn forwards(&self)->rusqlite::Result<Vec<Forward>> {let db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;read_forwards(&db)}
        pub fn create_forward(&self,f:&mut Forward)->rusqlite::Result<()> {let allowed=serde_json::to_string(&f.allowed_group_ids).map_err(|_|rusqlite::Error::InvalidQuery)?;let mut db=self.db.lock().map_err(|_|rusqlite::Error::InvalidQuery)?;let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;after_begin_immediate();ensure_setup(&tx)?;let name=normalized_name(&f.name)?;validate_group_refs(&tx,&f.allowed_group_ids)?;let target:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM peers WHERE id=?1)",[&f.target_peer_id],|r|r.get(0))?;if !target{return Err(sql_error("invalid target peer reference"));}insert_forward(&tx,f,&name,&allowed)?;tx.commit()?;f.name=name;Ok(())}
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
fn validate_group_refs(tx:&Transaction<'_>,refs:&[String])->rusqlite::Result<()> {for id in refs{let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",[id],|r|r.get(0))?;if !exists{return Err(sql_error("invalid group reference"));}}Ok(())}
fn settings_subnet(tx:&Transaction<'_>)->rusqlite::Result<Subnet24>{let raw:Option<String>=tx.query_row("SELECT subnet FROM network_settings WHERE id=1",[],|r|r.get(0)).optional()?;Subnet24::parse(&raw.ok_or_else(||sql_error("network setup is required before allocating addresses"))?).map_err(sql_error)}
fn ensure_setup(tx:&Transaction<'_>)->rusqlite::Result<()> { let _:String=tx.query_row("SELECT subnet FROM network_settings WHERE id=1",[],|r|r.get(0)).optional()?.ok_or_else(||sql_error("network setup is required"))?;Ok(()) }
fn normalized_name(name:&str)->rusqlite::Result<String>{let value=name.trim();if value.is_empty(){return Err(sql_error("name must not be empty"));}Ok(value.into())}
fn sql_error(message:impl Into<String>)->rusqlite::Error{rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput,message.into())))}
fn io_db(e:rusqlite::Error)->std::io::Error{std::io::Error::new(std::io::ErrorKind::Other,e.to_string())}

fn validate_current_schema(db:&Connection)->rusqlite::Result<()> {
    validate_schema_exact(db)?;
    let invalid:i64=db.query_row("SELECT COUNT(*) FROM forwards WHERE protocol NOT IN ('tcp','udp') OR target_port NOT BETWEEN 1 AND 65535",[],|r|r.get(0))?;
    if invalid != 0 { return Err(sql_error("current database contains an invalid forward protocol or port; refusing to modify database")); }
    Ok(())
}

fn schema_rows(db:&Connection)->rusqlite::Result<Vec<(String,String,String,Option<String>)>> {
    let mut q=db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name,sql")?;
    let rows=q.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?.collect();
    rows
}
fn validate_schema_exact(db:&Connection)->rusqlite::Result<()> {
    let reference=Connection::open_in_memory()?;
    reference.execute_batch(TABLES)?;
    if schema_rows(db)? != schema_rows(&reference)? { return Err(sql_error("current database schema differs from canonical schema; refusing to modify database")); }
    Ok(())
}
fn validate_current_data(db:&Connection)->rusqlite::Result<()> {
    let fk:Vec<(String,i64,String)>=db.prepare("PRAGMA foreign_key_check")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    if !fk.is_empty(){return Err(sql_error("current database contains foreign key violations; refusing to start"));}
    let identity_count:i64=db.query_row("SELECT COUNT(*) FROM hub_identity",[],|r|r.get(0))?;
    let invalid_identity:i64=db.query_row("SELECT COUNT(*) FROM hub_identity WHERE typeof(id)!='integer' OR id!=1 OR typeof(public_key)!='blob' OR length(public_key)!=32",[],|r|r.get(0))?;
    if invalid_identity!=0 || identity_count>1{return Err(sql_error("current database contains invalid hub identity; refusing to start"));}
    let configured:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)",[],|r|r.get(0))?;
    let inventory:i64=db.query_row("SELECT (SELECT COUNT(*) FROM groups)+(SELECT COUNT(*) FROM peers)+(SELECT COUNT(*) FROM forwards)",[],|r|r.get(0))?;
    if identity_count==0 && (configured || inventory!=0){return Err(sql_error("configured database is missing its hub identity; refusing to start"));}
    let invalid:i64=db.query_row("SELECT (SELECT COUNT(*) FROM groups WHERE typeof(id)!='text' OR typeof(name)!='text' OR typeof(allowed)!='text')+(SELECT COUNT(*) FROM peers WHERE typeof(id)!='text' OR typeof(name)!='text' OR typeof(public_key)!='text' OR typeof(ipv4)!='text' OR typeof(group_id)!='text' OR typeof(rx)!='integer' OR typeof(tx)!='integer' OR (last_handshake IS NOT NULL AND typeof(last_handshake)!='integer'))+(SELECT COUNT(*) FROM forwards WHERE typeof(id)!='text' OR typeof(name)!='text' OR typeof(protocol)!='text' OR typeof(target_peer_id)!='text' OR typeof(target_port)!='integer' OR typeof(allowed)!='text')",[],|r|r.get(0))?;
    if invalid!=0{return Err(sql_error("current database contains invalid inventory types; refusing to start"));}
    for (table,column) in [("groups","allowed"),("forwards","allowed")] {
        let sql=format!("SELECT {column} FROM {table}");let mut q=db.prepare(&sql)?;
        let values=q.query_map([],|r|r.get::<_,String>(0))?;
        for value in values { let raw=value?; let refs:Vec<String>=serde_json::from_str(&raw).map_err(|_|sql_error("current database contains malformed group references; refusing to start"))?;for id in refs { let found:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",[id],|r|r.get(0))?;if !found{return Err(sql_error("current database contains unknown group references; refusing to start"));} } }
    }
    Ok(())
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
}
