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
    let private=load_or_create_hub_key(Path::new(&key_path))?;
    let public=PublicKey::from(&StaticSecret::from(private));
    let store = Arc::new(Store::open(&path)?);
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

fn load_or_create_hub_key(path:&Path)->std::io::Result<[u8;32]> {
    match fs::symlink_metadata(path) {
        Ok(meta)=>{
            if !meta.file_type().is_file(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key must be a regular, non-symlink file"))}
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; if meta.permissions().mode()&0o777!=0o600{return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"existing hub key must have mode 0600"))} }
            let mut file=File::open(path)?;
            let opened=file.metadata()?;
            if !opened.file_type().is_file(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key must be regular file"))}
            #[cfg(unix)] { use std::os::unix::fs::MetadataExt; if meta.dev()!=opened.dev()||meta.ino()!=opened.ino(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key changed while opening"))} }
            let mut bytes=Vec::new();file.read_to_end(&mut bytes)?;
            if bytes.len()!=32{return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"existing hub key must be exactly 32 bytes; refusing to rotate"))}
            let mut key=[0u8;32];key.copy_from_slice(&bytes);Ok(key)
        }
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
            let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
            let mut options=OpenOptions::new();options.write(true).create_new(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
            let mut file=options.open(path)?;file.write_all(&key)?;file.sync_all()?;Ok(key)
        }
        Err(e)=>Err(e),
    }
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
}

// Avoid imposing an additional error-reporting dependency on the executable.
mod anyhow_placeholder {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}
