use crate::{api, hub_key, kernel, storage::Store};
use boringtun::x25519::{PublicKey, StaticSecret};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

pub(crate) async fn run() -> Result<()> {
    let port: u16 = match env::var("WIREHUB_PORT") {
        Ok(v) => v
            .parse()
            .map_err(|_| "WIREHUB_PORT must be an integer from 1 through 65535")?,
        Err(_) => 51820,
    };
    if port == 0 {
        return Err("WIREHUB_PORT must be nonzero".into());
    }
    let bind: Ipv4Addr = match env::var("WIREHUB_HTTP_BIND") {
        Ok(v) => v
            .parse()
            .map_err(|_| "WIREHUB_HTTP_BIND must be an IPv4 address")?,
        Err(_) => Ipv4Addr::LOCALHOST,
    };
    let path = env::var("WIREHUB_DB").unwrap_or_else(|_| "wirehub.sqlite3".into());
    let token = env::var("WIREHUB_ADMIN_TOKEN").map_err(|_| "WIREHUB_ADMIN_TOKEN is required")?;
    if token.trim().is_empty() {
        return Err("WIREHUB_ADMIN_TOKEN must not be empty".into());
    }
    let proxy_mode = env::var("WIREHUB_TRUSTED_PROXY_MODE")
        .map(|v| v == "1")
        .unwrap_or(false);
    if !bind.is_loopback() && !proxy_mode {
        return Err("non-loopback HTTP bind requires WIREHUB_TRUSTED_PROXY_MODE=1 behind a trusted TLS/auth proxy".into());
    }
    let key_path = env::var("WIREHUB_HUB_KEY").unwrap_or_else(|_| "wirehub.key".into());
    let store = Arc::new(Store::open_service(Path::new(&path))?);
    let private = hub_key::load_or_bind_hub_key(&store, Path::new(&key_path))?;
    store.recover_pending_provisions()?;
    let public = PublicKey::from(&StaticSecret::from(private));
    let udp = tokio::net::UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?;
    let loader_store = store.clone();
    let (kernel, handle) = kernel::Kernel::initialize(udp, private, move || {
        let raw = loader_store
            .runtime_snapshot()
            .map_err(|_| kernel::SnapshotLoadError)?;
        crate::kernel::snapshot::CompiledSnapshot::try_from(raw)
            .map_err(|_| kernel::SnapshotLoadError)
    })
    .await?;
    let state = api::AppState {
        store: store.clone(),
        token: Some(token),
        hub_public: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            public.as_bytes(),
        ),
        kernel: handle,
    };
    let app = api::router(state);
    let tcp = tokio::net::TcpListener::bind(SocketAddr::from((bind, port))).await?;
    let kernel_task = tokio::spawn(kernel.run());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    supervise(
        kernel_task,
        async move {
            axum::serve(tcp, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
        },
        termination_signal(),
        shutdown_tx,
        SHUTDOWN_TIMEOUT,
    )
    .await
}

async fn termination_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

#[cfg(test)]
async fn supervise_kernel_and_http<F>(
    kernel_task: tokio::task::JoinHandle<std::result::Result<(), kernel::RunError>>,
    http: F,
) -> Result<()>
where
    F: std::future::Future<Output = std::io::Result<()>>,
{
    let (tx, _) = tokio::sync::oneshot::channel();
    supervise(
        kernel_task,
        http,
        std::future::pending(),
        tx,
        SHUTDOWN_TIMEOUT,
    )
    .await
}

async fn supervise<F, S>(
    mut kernel_task: tokio::task::JoinHandle<std::result::Result<(), kernel::RunError>>,
    http: F,
    shutdown: S,
    trigger: tokio::sync::oneshot::Sender<()>,
    deadline: std::time::Duration,
) -> Result<()>
where
    F: std::future::Future<Output = std::io::Result<()>>,
    S: std::future::Future<Output = std::io::Result<()>>,
{
    tokio::pin!(http, shutdown);
    tokio::select! {
        result=&mut kernel_task=>match result{Ok(Ok(()))|Ok(Err(_))=>Err("kernel exited unexpectedly".into()),Err(_)=>Err("kernel task panicked".into())},
        result=&mut http=>{kernel_task.abort();let _=kernel_task.await;result?;Ok(())},
        signal=&mut shutdown=>{
            let _ = trigger.send(());
            // UDP must remain alive until acknowledged management writes finish.
            let drained = tokio::time::timeout(deadline, &mut http).await;
            kernel_task.abort(); let _ = kernel_task.await;
            signal?;
            drained.map_err(|_| "HTTP graceful shutdown exceeded the drain deadline")??;
            Ok(())
        }
    }
}

#[cfg(test)]
mod supervision_tests {
    use super::*;
    use crate::kernel::{ReloadError, RunError, SnapshotLoadError, StartError};
    use std::{future::pending, io};
    use tokio::sync::oneshot;
    struct DropSignal(Option<oneshot::Sender<()>>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }
    async fn pending_http(tx: oneshot::Sender<()>) -> io::Result<()> {
        let _dropped = DropSignal(Some(tx));
        pending().await
    }
    async fn assert_kernel_exit(
        task: tokio::task::JoinHandle<std::result::Result<(), kernel::RunError>>,
        expected: &str,
    ) {
        let (http_dropped_tx, http_dropped_rx) = oneshot::channel();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            supervise_kernel_and_http(task, pending_http(http_dropped_tx)),
        )
        .await
        .expect("supervisor timed out");
        assert_eq!(result.unwrap_err().to_string(), expected);
        tokio::time::timeout(std::time::Duration::from_secs(1), http_dropped_rx)
            .await
            .expect("HTTP future was not dropped")
            .unwrap();
    }
    #[tokio::test]
    async fn kernel_normal_exit_and_error_both_stop_http() {
        assert_kernel_exit(tokio::spawn(async { Ok(()) }), "kernel exited unexpectedly").await;
        assert_kernel_exit(
            tokio::spawn(async { Err(kernel::RunError::Stopped) }),
            "kernel exited unexpectedly",
        )
        .await;
    }
    #[tokio::test]
    async fn kernel_panic_stops_http() {
        assert_kernel_exit(
            tokio::spawn(async {
                panic!("stub kernel panic");
                #[allow(unreachable_code)]
                Ok(())
            }),
            "kernel task panicked",
        )
        .await;
    }
    async fn assert_http_completion(result: io::Result<()>, should_succeed: bool) {
        let (udp_dropped_tx, udp_dropped_rx) = oneshot::channel();
        let (udp_started_tx, udp_started_rx) = oneshot::channel();
        let udp = tokio::spawn(async move {
            let _dropped = DropSignal(Some(udp_dropped_tx));
            let _ = udp_started_tx.send(());
            pending::<std::result::Result<(), kernel::RunError>>().await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), udp_started_rx)
            .await
            .expect("kernel task did not start")
            .unwrap();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            supervise_kernel_and_http(udp, async move { result }),
        )
        .await
        .expect("supervisor timed out");
        tokio::time::timeout(std::time::Duration::from_secs(1), udp_dropped_rx)
            .await
            .expect("kernel task was not cancelled")
            .unwrap();
        match (outcome, should_succeed) {
            (Ok(()), true) => {}
            (Err(error), false) => assert_eq!(error.to_string(), "stub HTTP failure"),
            (other, _) => panic!("unexpected HTTP supervisor result: {other:?}"),
        }
    }
    #[tokio::test]
    async fn http_success_and_failure_abort_and_finish_kernel() {
        assert_http_completion(Ok(()), true).await;
        assert_http_completion(Err(io::Error::other("stub HTTP failure")), false).await;
    }

    fn assert_error_traits<T: std::error::Error + Send + Sync + 'static>() {}
    #[test]
    fn startup_and_kernel_errors_are_send_sync_static_errors() {
        assert_error_traits::<SnapshotLoadError>();
        assert_error_traits::<ReloadError>();
        assert_error_traits::<StartError>();
        assert_error_traits::<RunError>();
    }

    fn empty_snapshot() -> crate::kernel::CompiledSnapshot {
        crate::kernel::CompiledSnapshot::try_from(crate::model::NetworkSnapshot {
            revision: 0,
            settings: None,
            groups: vec![],
            peers: vec![],
            forwards: vec![],
        })
        .unwrap()
    }
    async fn initialized_kernel(
        loader_panics_on_reload: bool,
    ) -> (crate::kernel::Kernel, crate::kernel::KernelHandle) {
        let socket = tokio::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let mut loads = 0;
        crate::kernel::Kernel::initialize(socket, [7; 32], move || {
            loads += 1;
            if loader_panics_on_reload && loads > 1 {
                panic!("reload loader panic")
            }
            Ok(empty_snapshot())
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn http_completion_aborts_initialized_kernel_and_clears_readiness() {
        for (result, success) in [
            (Ok(()), true),
            (Err(io::Error::other("real HTTP failure")), false),
        ] {
            let (kernel, handle) = initialized_kernel(false).await;
            assert!(handle.is_ready());
            let task = tokio::spawn(kernel.run());
            let outcome = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                supervise_kernel_and_http(task, async move { result }),
            )
            .await
            .expect("supervisor timed out");
            match (outcome, success) {
                (Ok(()), true) => {}
                (Err(error), false) => assert_eq!(error.to_string(), "real HTTP failure"),
                (other, _) => panic!("unexpected supervisor result: {other:?}"),
            }
            assert!(
                !handle.is_ready(),
                "aborted initialized kernel must clear readiness"
            );
        }
    }
    #[tokio::test]
    async fn reload_loader_panic_stops_supervision_and_drops_http() {
        let (kernel, handle) = initialized_kernel(true).await;
        assert!(handle.is_ready());
        let task = tokio::spawn(kernel.run());
        let (http_dropped_tx, http_dropped_rx) = oneshot::channel();
        let http = pending_http(http_dropped_tx);
        let reload = tokio::spawn({
            let handle = handle.clone();
            async move { handle.reload().await }
        });
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            supervise_kernel_and_http(task, http),
        )
        .await
        .expect("supervisor timed out");
        assert_eq!(outcome.unwrap_err().to_string(), "kernel task panicked");
        tokio::time::timeout(std::time::Duration::from_secs(1), reload)
            .await
            .expect("reload timed out")
            .unwrap()
            .expect_err("panicking reload must fail");
        tokio::time::timeout(std::time::Duration::from_secs(1), http_dropped_rx)
            .await
            .expect("HTTP future was not dropped")
            .unwrap();
        assert!(!handle.is_ready(), "panicked kernel must clear readiness");
    }
    #[tokio::test]
    async fn shutdown_keeps_kernel_alive_until_http_drains_then_clears_readiness() {
        let (kernel, handle) = initialized_kernel(false).await;
        let task = tokio::spawn(kernel.run());
        let (trigger, receiver) = oneshot::channel();
        let observed = handle.clone();
        let http = async move {
            receiver.await.unwrap();
            assert!(observed.is_ready(), "UDP stays active during HTTP drain");
            observed.reload().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            assert!(observed.is_ready());
            Ok(())
        };
        supervise(
            task,
            http,
            async { Ok(()) },
            trigger,
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(!handle.is_ready());
    }

    #[tokio::test]
    async fn shutdown_deadline_bounds_stuck_http_and_stops_kernel() {
        let (kernel, handle) = initialized_kernel(false).await;
        let task = tokio::spawn(kernel.run());
        let (trigger, _) = oneshot::channel();
        let (dropped, dropped_rx) = oneshot::channel();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            supervise(
                task,
                pending_http(dropped),
                async { Ok(()) },
                trigger,
                std::time::Duration::from_millis(20),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "HTTP graceful shutdown exceeded the drain deadline"
        );
        assert!(!handle.is_ready());
        dropped_rx.await.unwrap();
    }
}
