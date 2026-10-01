mod api;
mod model;
mod flows;
mod network;
mod policy;
mod storage;
mod static_assets;
mod transport;

use std::{env, fs::{self, File, OpenOptions}, io::{Read, Write}, net::{Ipv4Addr, SocketAddr}, path::Path, sync::Arc};
use axum::{routing::{get, put, delete}, Router};
use boringtun::x25519::{PublicKey, StaticSecret};
use rand::{rngs::OsRng, RngCore};
use storage::Store;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow_placeholder::Result<()> {
    if env::args().any(|a| a == "export-openapi") {
        print!("{}", api::openapi());
        return Ok(());
    }
    let port: u16 = match env::var("WIREHUB_PORT") { Ok(v)=>v.parse().map_err(|_|"WIREHUB_PORT must be an integer from 1 through 65535")?, Err(_)=>51820 };
    if port==0{return Err("WIREHUB_PORT must be nonzero".into())}
    let bind: Ipv4Addr = match env::var("WIREHUB_HTTP_BIND") { Ok(v)=>v.parse().map_err(|_|"WIREHUB_HTTP_BIND must be an IPv4 address")?, Err(_)=>Ipv4Addr::LOCALHOST };
    let path = env::var("WIREHUB_DB").unwrap_or_else(|_| "wirehub.sqlite3".into());
    let token=env::var("WIREHUB_ADMIN_TOKEN").map_err(|_|"WIREHUB_ADMIN_TOKEN is required")?;
    if token.trim().is_empty(){return Err("WIREHUB_ADMIN_TOKEN must not be empty".into())}
    // Proxy mode assumes an independently authenticated/TLS-terminating trusted proxy protects this HTTP listener.
    let proxy_mode=env::var("WIREHUB_TRUSTED_PROXY_MODE").map(|v|v=="1").unwrap_or(false);
    if !bind.is_loopback() && !proxy_mode{return Err("non-loopback HTTP bind requires WIREHUB_TRUSTED_PROXY_MODE=1 behind a trusted TLS/auth proxy".into())}
    let key_path=env::var("WIREHUB_HUB_KEY").unwrap_or_else(|_| "wirehub.key".into());
    let store = Arc::new(Store::open(&path)?);
    let private=load_or_bind_hub_key(&store,Path::new(&key_path))?;
    let public=PublicKey::from(&StaticSecret::from(private));
    let (reload_tx, reload_rx) = mpsc::channel(16);
    let runtime_stats = transport::RuntimeStats::default();
    let state = api::AppState { store: store.clone(), token:Some(token), hub_public:base64::Engine::encode(&base64::engine::general_purpose::STANDARD,public.as_bytes()), reload_tx, runtime_stats: runtime_stats.clone() };
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/setup", get(api::get_setup).post(api::post_setup))
        .route("/api/settings", put(api::put_settings))
        .route("/api/groups", get(api::list_groups).post(api::create_group))
        .route("/api/groups/:id", delete(api::delete_group))
        .route("/api/groups/:id/acl", put(api::set_acl))
        .route("/api/peers", get(api::list_peers).post(api::create_peer))
        .route("/api/peers/:id", delete(api::delete_peer))
        .route("/api/peers/:id/group", put(api::move_peer))
        .route("/api/forwards", get(api::list_forwards).post(api::create_forward))
        .route("/api/forwards/:id", delete(api::delete_forward))
        .fallback(static_assets::handler)
        .with_state(state);
    let tcp = tokio::net::TcpListener::bind(SocketAddr::from((bind, port))).await?;
    let udp = tokio::net::UdpSocket::bind(SocketAddr::from(([0,0,0,0], port))).await?;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(transport::run_udp(udp, store, private, reload_rx, runtime_stats, Some(ready_tx)));
    ready_rx.await.map_err(|_| "UDP router failed during startup")?.map_err(|_| "failed to load persisted peers; refusing to start")?;
    axum::serve(tcp, app).await?;
    Ok(())
}

#[cfg(test)]
fn load_or_create_hub_key(path:&Path)->std::io::Result<[u8;32]> {
    match fs::symlink_metadata(path) {
        Ok(_)=>load_hub_key(path),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
            let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
            publish_hub_key(path,&key)
        }
        Err(e)=>Err(e),
    }
}
#[cfg(test)]
fn load_hub_key(path:&Path)->std::io::Result<[u8;32]> {
    load_hub_key_inner(path,false)
}
fn load_hub_key_synced(path:&Path)->std::io::Result<[u8;32]> {
    load_hub_key_inner(path,true)
}
fn load_hub_key_inner(path:&Path,sync:bool)->std::io::Result<[u8;32]> {
    let meta=fs::symlink_metadata(path)?;
    if !meta.file_type().is_file(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key must be a regular, non-symlink file"))}
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;if meta.permissions().mode()&0o777!=0o600{return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"existing hub key must have mode 0600"))}}
    let file=File::open(path)?;let opened=file.metadata()?;
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;if meta.dev()!=opened.dev()||meta.ino()!=opened.ino(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key changed while opening"))}}
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;if opened.permissions().mode()&0o777!=0o600{return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"existing hub key must have mode 0600"))}}
    let mut bytes=Vec::new();(&file).take(33).read_to_end(&mut bytes)?;if bytes.len()!=32{return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"existing hub key must be exactly 32 bytes; refusing to rotate"))}if sync { file.sync_all()?; } Ok(bytes.try_into().unwrap())
}
fn load_or_bind_hub_key(store:&Store,path:&Path)->std::io::Result<[u8;32]> {
    store.bootstrap_hub_identity(||match fs::symlink_metadata(path){Ok(_)=>load_hub_key_synced(path),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{let mut key=[0;32];OsRng.fill_bytes(&mut key);Ok(key)},Err(e)=>Err(e)},|_|load_hub_key_synced(path),|key|publish_hub_key(path,key))
        .map_err(|e|std::io::Error::new(std::io::ErrorKind::InvalidData,e.to_string()))
}

fn publish_hub_key(path:&Path,key:&[u8;32])->std::io::Result<[u8;32]> {
    publish_hub_key_with(path,key,|_|Ok(()))
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum PublishCheckpoint { TempSynced, Linked, CollisionWinnerSynced }
fn publish_hub_key_with<F>(path:&Path,key:&[u8;32],mut checkpoint:F)->std::io::Result<[u8;32]>
where F:FnMut(PublishCheckpoint)->std::io::Result<()> {
    let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or_else(||Path::new("."));
    let name=path.file_name().ok_or_else(||std::io::Error::new(std::io::ErrorKind::InvalidInput,"hub key path has no filename"))?;
    let temp_path;
    let mut options=OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let (mut file,p)=loop {let candidate=parent.join(format!(".{}.{}.tmp",name.to_string_lossy(),hex::encode(rand::random::<[u8;8]>())));match options.open(&candidate){Ok(f)=>break(f,candidate),Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(e)}};
    temp_path=p;
    let result=(||{file.write_all(key)?;file.sync_all()?;checkpoint(PublishCheckpoint::TempSynced)?;drop(file);
        let published=match fs::hard_link(&temp_path,path){Ok(())=>{checkpoint(PublishCheckpoint::Linked)?;*key},Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{let winner=load_hub_key_synced(path)?;checkpoint(PublishCheckpoint::CollisionWinnerSynced)?;winner},Err(e)=>return Err(e)};
        fs::remove_file(&temp_path)?;
        #[cfg(unix)] {File::open(parent)?.sync_all()?;}
        Ok(published)})();
    if result.is_err(){let _=fs::remove_file(&temp_path);} result
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn hub_key_is_exclusive_persistent_and_rejects_malformed_existing_file() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("hub.key");
        let first=load_or_create_hub_key(&path).unwrap();assert_eq!(first.len(),32);
        assert_eq!(load_or_create_hub_key(&path).unwrap(),first);
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt;assert_eq!(fs::metadata(&path).unwrap().permissions().mode()&0o777,0o600); }
        fs::write(&path,b"bad").unwrap();assert!(load_or_create_hub_key(&path).is_err());assert_eq!(fs::read(&path).unwrap(),b"bad");
    }
    #[cfg(unix)]
    #[test]
    fn existing_symlink_and_insecure_key_are_rejected() {
        use std::os::unix::fs::{symlink,PermissionsExt};
        let dir=tempfile::tempdir().unwrap();let key=dir.path().join("real");fs::write(&key,[3u8;32]).unwrap();fs::set_permissions(&key,fs::Permissions::from_mode(0o600)).unwrap();
        let link=dir.path().join("link");symlink(&key,&link).unwrap();assert!(load_or_create_hub_key(&link).is_err());
        fs::set_permissions(&key,fs::Permissions::from_mode(0o644)).unwrap();assert!(load_or_create_hub_key(&key).is_err());
    }
    #[test]
    fn startup_identity_restart_and_uncommitted_key_recovery_are_paired() {
        let dir=tempfile::tempdir().unwrap();let db=dir.path().join("db");let key=dir.path().join("hub.key");
        let store=Store::open(db.to_str().unwrap()).unwrap();let first=load_or_bind_hub_key(&store,&key).unwrap();assert_eq!(store.hub_identity().unwrap().unwrap(),*PublicKey::from(&StaticSecret::from(first)).as_bytes());
        assert_eq!(load_or_bind_hub_key(&store,&key).unwrap(),first);fs::remove_file(&key).unwrap();assert!(load_or_bind_hub_key(&store,&key).is_err());assert!(!key.exists());
        let interrupted=dir.path().join("interrupted.sqlite");let interrupted_key=dir.path().join("interrupted.key");let orphan=load_or_create_hub_key(&interrupted_key).unwrap();let unbound=Store::open(interrupted.to_str().unwrap()).unwrap();assert_eq!(load_or_bind_hub_key(&unbound,&interrupted_key).unwrap(),orphan);assert!(unbound.hub_identity().unwrap().is_some());
    }
    #[test]
    fn key_reader_rejects_oversized_file_after_short_reads() {
        let dir=tempfile::tempdir().unwrap(); let path=dir.path().join("oversized");
        fs::write(&path,[9u8;33]).unwrap();
        assert!(load_hub_key(&path).is_err());
    }
    #[test]
    fn relative_key_path_can_be_published() {
        let path=Path::new("wirehub-phase1-relative-test.key");
        let _=fs::remove_file(path);
        assert!(load_or_create_hub_key(path).is_ok());
        let _=fs::remove_file(path);
    }
    #[test]
    fn missing_key_for_bound_unconfigured_database_does_not_rotate_identity() {
        let dir=tempfile::tempdir().unwrap(); let db=dir.path().join("db"); let key=dir.path().join("key");
        let store=Store::open(db.to_str().unwrap()).unwrap();
        store.bind_hub_identity(&[4;32]).unwrap();
        assert!(load_or_bind_hub_key(&store,&key).is_err());
        assert!(!key.exists()); assert_eq!(store.hub_identity().unwrap(),Some([4;32]));
    }
    #[test]
    fn bootstrap_publication_failure_boundaries_preserve_recoverable_pairing() {
        let dir=tempfile::tempdir().unwrap();let db=dir.path().join("boundary.sqlite");let path=dir.path().join("hub.key");
        let store=Store::open(db.to_str().unwrap()).unwrap();let proposed=[31u8;32];
        let mut before_stages=Vec::new();let before=store.bootstrap_hub_identity(||Ok(proposed),|_|unreachable!(),|key|publish_hub_key_with(&path,key,|stage|{before_stages.push(stage);if stage==PublishCheckpoint::TempSynced{Err(std::io::Error::other("before link"))}else{Ok(())}}));
        assert!(before.is_err());assert_eq!(before_stages,vec![PublishCheckpoint::TempSynced]);assert!(!path.exists());assert_eq!(store.hub_identity().unwrap(),None);assert_eq!(fs::read_dir(dir.path()).unwrap().count(),1);
        let mut after_stages=Vec::new();let after=store.bootstrap_hub_identity(||Ok(proposed),|_|unreachable!(),|key|publish_hub_key_with(&path,key,|stage|{after_stages.push(stage);if stage==PublishCheckpoint::Linked{Err(std::io::Error::other("after link"))}else{Ok(())}}));
        assert!(after.is_err());assert_eq!(after_stages,vec![PublishCheckpoint::TempSynced,PublishCheckpoint::Linked]);assert_eq!(fs::read(&path).unwrap(),proposed);assert_eq!(store.hub_identity().unwrap(),None);assert_eq!(fs::read_dir(dir.path()).unwrap().count(),2);
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;assert_eq!(fs::metadata(&path).unwrap().permissions().mode()&0o777,0o600);}
        drop(store);let restarted=Store::open(db.to_str().unwrap()).unwrap();
        assert_eq!(load_or_bind_hub_key(&restarted,&path).unwrap(),proposed);assert!(restarted.hub_identity().unwrap().is_some());
    }
    #[test]
    fn key_publication_collision_keeps_complete_winner_without_overwrite() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("winner.key");let winner=[51u8;32];let contender=[52u8;32];
        fs::write(&path,winner).unwrap();
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap();}
        let mut stages=Vec::new();assert_eq!(publish_hub_key_with(&path,&contender,|stage|{stages.push(stage);Ok(())}).unwrap(),winner);assert_eq!(fs::read(&path).unwrap(),winner);assert_eq!(stages,vec![PublishCheckpoint::TempSynced,PublishCheckpoint::CollisionWinnerSynced]);
    }
}

// Avoid imposing an additional error-reporting dependency on the executable.
mod anyhow_placeholder {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}
