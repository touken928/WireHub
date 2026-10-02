use std::{env, net::{Ipv4Addr, SocketAddr}, path::Path, sync::Arc};
use boringtun::x25519::{PublicKey, StaticSecret};
use crate::{api, hub_key, storage::Store, transport};
use tokio::sync::mpsc;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) async fn run() -> Result<()> {
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
    let private=hub_key::load_or_bind_hub_key(&store,Path::new(&key_path))?;
    store.recover_pending_provisions()?;
    let public=PublicKey::from(&StaticSecret::from(private));
    let (reload_tx, reload_rx) = mpsc::channel(16);
    let runtime_stats = transport::RuntimeStats::default();
    let readiness=transport::Readiness::default();
    let state = api::AppState { store: store.clone(), token:Some(token), hub_public:base64::Engine::encode(&base64::engine::general_purpose::STANDARD,public.as_bytes()), reload_tx, runtime_stats: runtime_stats.clone(), readiness:readiness.clone() };
    let app = api::router(state);
    let tcp = tokio::net::TcpListener::bind(SocketAddr::from((bind, port))).await?;
    let udp = tokio::net::UdpSocket::bind(SocketAddr::from(([0,0,0,0], port))).await?;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let udp_task=tokio::spawn(transport::run_udp(udp, store, private, reload_rx, runtime_stats, readiness.clone(), Some(ready_tx)));
    match ready_rx.await {
        Err(_) => return Err("UDP router failed during startup".into()),
        Ok(Err(())) => return Err("failed to load persisted peers; refusing to start".into()),
        Ok(Ok(())) => {}
    }
    supervise_udp_and_http(udp_task, async move { axum::serve(tcp, app).await }, readiness).await
}

async fn supervise_udp_and_http<F>(
    mut udp_task: tokio::task::JoinHandle<std::result::Result<(), ()>>,
    http: F,
    readiness: transport::Readiness,
) -> Result<()>
where
    F: std::future::Future<Output = std::io::Result<()>>,
{
    tokio::pin!(http);
    tokio::select! {
        result = &mut udp_task => {
            readiness.set(false);
            match result {
                Ok(Ok(())) | Ok(Err(())) => Err("UDP router exited unexpectedly".into()),
                Err(_) => Err("UDP router task panicked".into()),
            }
        }
        result = &mut http => {
            readiness.set(false);
            udp_task.abort();
            // Await the aborted task so its future has been dropped before returning.
            let _ = udp_task.await;
            result?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod supervision_tests {
    use super::*;
    use std::{future::{pending, Future}, io};
    use tokio::sync::oneshot;

    struct DropSignal(Option<oneshot::Sender<()>>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() { let _ = tx.send(()); }
        }
    }

    fn pending_http(tx: oneshot::Sender<()>) -> impl Future<Output = io::Result<()>> {
        async move {
            let _dropped = DropSignal(Some(tx));
            pending().await
        }
    }

    async fn assert_udp_exit(task: tokio::task::JoinHandle<std::result::Result<(), ()>>, expected: &str) {
        let readiness = transport::Readiness::default();
        readiness.set(true);
        let (http_dropped_tx, http_dropped_rx) = oneshot::channel();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            supervise_udp_and_http(task, pending_http(http_dropped_tx), readiness.clone()),
        ).await.expect("supervisor timed out");
        assert_eq!(result.unwrap_err().to_string(), expected);
        assert!(!readiness.is_ready());
        tokio::time::timeout(std::time::Duration::from_secs(1), http_dropped_rx)
            .await.expect("HTTP future was not dropped").unwrap();
    }

    #[tokio::test]
    async fn udp_normal_exit_and_error_both_stop_http() {
        assert_udp_exit(tokio::spawn(async { Ok(()) }), "UDP router exited unexpectedly").await;
        assert_udp_exit(tokio::spawn(async { Err(()) }), "UDP router exited unexpectedly").await;
    }

    #[tokio::test]
    async fn udp_panic_stops_http() {
        assert_udp_exit(tokio::spawn(async { panic!("stub UDP panic"); #[allow(unreachable_code)] Ok(()) }), "UDP router task panicked").await;
    }

    async fn assert_http_completion(result: io::Result<()>, should_succeed: bool) {
        let readiness = transport::Readiness::default();
        readiness.set(true);
        let (udp_dropped_tx, udp_dropped_rx) = oneshot::channel();
        let (udp_started_tx, udp_started_rx) = oneshot::channel();
        let udp = tokio::spawn(async move {
            let _dropped = DropSignal(Some(udp_dropped_tx));
            let _ = udp_started_tx.send(());
            pending::<std::result::Result<(), ()>>().await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), udp_started_rx)
            .await.expect("UDP task did not start").unwrap();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            supervise_udp_and_http(udp, async move { result }, readiness.clone()),
        ).await.expect("supervisor timed out");
        assert!(!readiness.is_ready());
        tokio::time::timeout(std::time::Duration::from_secs(1), udp_dropped_rx)
            .await.expect("UDP task was not cancelled").unwrap();
        match (outcome, should_succeed) {
            (Ok(()), true) => {}
            (Err(error), false) => assert_eq!(error.to_string(), "stub HTTP failure"),
            (other, _) => panic!("unexpected HTTP supervisor result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_success_and_failure_abort_and_finish_udp() {
        assert_http_completion(Ok(()), true).await;
        assert_http_completion(Err(io::Error::other("stub HTTP failure")), false).await;
    }
}
