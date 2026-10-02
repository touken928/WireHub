use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use base64::Engine;
use tokio::{net::UdpSocket, sync::Notify, task::JoinHandle, time};

use crate::{
    kernel::{CompiledSnapshot, Kernel, ReloadError, RunError, SnapshotLoadError, StartError},
    model::{Group, NetworkSettings, NetworkSnapshot, Peer},
};

const WAIT: Duration = Duration::from_secs(5);

fn snapshot(id: &str, bytes: u64) -> CompiledSnapshot {
    let raw = NetworkSnapshot {
        settings: Some(NetworkSettings {
            subnet: "10.9.0.0/24".into(),
            endpoint: "127.0.0.1:51820".into(),
            persistent_keepalive: 25,
        }),
        groups: vec![Group {
            id: "group".into(),
            name: "group".into(),
            allowed_groups: vec![],
        }],
        peers: vec![Peer {
            id: id.into(),
            name: id.into(),
            public_key: base64::engine::general_purpose::STANDARD.encode([bytes as u8; 32]),
            ipv4: "10.9.0.2".into(),
            group_id: "group".into(),
            received_bytes: bytes,
            sent_bytes: bytes + 1,
            last_handshake_unix: Some(bytes as i64),
        }],
        forwards: vec![],
    };
    CompiledSnapshot::try_from(raw).expect("fixture snapshot compiles")
}

async fn kernel<F>(loader: F) -> (Kernel, super::super::KernelHandle)
where
    F: FnMut() -> Result<CompiledSnapshot, SnapshotLoadError> + Send + 'static,
{
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    Kernel::initialize(socket, [0; 32], loader).await.unwrap()
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    time::timeout(WAIT, async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("condition timed out");
}

#[tokio::test]
async fn cancel_queued_reload_still_loads_latest_snapshot_when_run_starts() {
    let version = Arc::new(AtomicUsize::new(1));
    let loader_version = version.clone();
    let (kernel, handle) = kernel(move || {
        let value = loader_version.load(Ordering::Acquire);
        Ok(snapshot(&format!("peer-{value}"), value as u64))
    })
    .await;

    let caller_handle = handle.clone();
    let caller = tokio::spawn(async move { caller_handle.reload().await });
    wait_until(|| handle.commands.capacity() < 16).await;
    caller.abort();
    let _ = caller.await;
    version.store(2, Ordering::Release);

    let run = tokio::spawn(kernel.run());
    let stats = time::timeout(WAIT, async {
        loop {
            let stats = handle.stats().await;
            if stats.contains_key("peer-2") {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("queued command did not apply latest loader version");
    assert_eq!(stats.get("peer-2").unwrap().rx_bytes, 2);
    assert!(!stats.contains_key("peer-1"));
    assert!(handle.is_ready());
    drop(handle);
    assert!(matches!(run.await.unwrap(), Err(RunError::Stopped)));
}

#[tokio::test]
async fn ack_timeout_does_not_remove_queued_reload() {
    let (kernel, handle) = kernel(|| Ok(snapshot("after-timeout", 31))).await;
    assert_eq!(
        time::timeout(Duration::from_secs(4), handle.reload())
            .await
            .unwrap(),
        Err(ReloadError::Timeout)
    );
    assert!(
        handle.commands.capacity() < 16,
        "timed-out command must remain queued"
    );

    let run = tokio::spawn(kernel.run());
    let stats = time::timeout(WAIT, async {
        loop {
            let stats = handle.stats().await;
            if stats.contains_key("after-timeout") {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("queued timed-out reload was not applied");
    assert_eq!(stats.get("after-timeout").unwrap().rx_bytes, 31);
    assert!(handle.is_ready());
    drop(handle);
    assert!(matches!(run.await.unwrap(), Err(RunError::Stopped)));
}

#[tokio::test]
async fn full_queue_rejects_seventeenth_without_leaking_callers() {
    let (kernel, handle) = kernel(|| Ok(snapshot("peer", 1))).await;
    let callers: Vec<JoinHandle<Result<(), ReloadError>>> = (0..16)
        .map(|_| {
            let handle = handle.clone();
            tokio::spawn(async move { handle.reload().await })
        })
        .collect();
    wait_until(|| handle.commands.capacity() == 0).await;

    assert_eq!(handle.reload().await, Err(ReloadError::Timeout));
    assert_eq!(
        handle.commands.capacity(),
        0,
        "failed send must not enqueue"
    );

    for caller in &callers {
        caller.abort();
    }
    for caller in callers {
        assert!(caller.await.unwrap_err().is_cancelled());
    }
    let readiness = handle.readiness.clone();
    drop(handle);
    assert!(matches!(kernel.run().await, Err(RunError::Stopped)));
    assert!(!readiness.load(Ordering::Acquire));
}

#[tokio::test]
async fn rejected_reload_fails_closed_and_later_valid_reload_recovers() {
    let call = Arc::new(AtomicUsize::new(0));
    let loader_call = call.clone();
    let (kernel, handle) = kernel(move || match loader_call.fetch_add(1, Ordering::AcqRel) {
        0 => Ok(snapshot("initial", 1)),
        1 => Err(SnapshotLoadError),
        _ => Ok(snapshot("latest", 42)),
    })
    .await;
    let run = tokio::spawn(kernel.run());

    assert_eq!(handle.reload().await, Err(ReloadError::Rejected));
    assert!(!handle.is_ready());
    assert!(handle.stats().await.is_empty());
    assert_eq!(handle.reload().await, Ok(()));
    let stats = handle.stats().await;
    assert_eq!(stats.get("latest").unwrap().rx_bytes, 42);
    assert!(!stats.contains_key("initial"));
    assert!(handle.is_ready());

    drop(handle);
    assert!(matches!(run.await.unwrap(), Err(RunError::Stopped)));
}

#[tokio::test]
async fn readiness_is_published_only_after_stats_on_recovery() {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    let loader_calls = calls.clone();
    let loader_entered = entered.clone();
    let (kernel, handle) = kernel(move || match loader_calls.fetch_add(1, Ordering::AcqRel) {
        0 => Ok(snapshot("initial", 1)),
        1 => Err(SnapshotLoadError),
        _ => {
            loader_entered.notify_one();
            Ok(snapshot("recovered", 55))
        }
    })
    .await;
    let run = tokio::spawn(kernel.run());

    assert_eq!(handle.reload().await, Err(ReloadError::Rejected));
    assert!(!handle.is_ready());
    let stats_guard = time::timeout(WAIT, handle.stats.write())
        .await
        .expect("could not lock published stats");
    let reload_handle = handle.clone();
    let reload = tokio::spawn(async move { reload_handle.reload().await });
    time::timeout(WAIT, entered.notified())
        .await
        .expect("successful reload did not enter loader");
    wait_until(|| calls.load(Ordering::Acquire) >= 3).await;
    tokio::task::yield_now().await;
    assert!(
        !handle.is_ready(),
        "readiness must wait for blocked stats publish"
    );
    drop(stats_guard);

    assert_eq!(time::timeout(WAIT, reload).await.unwrap().unwrap(), Ok(()));
    assert!(handle.is_ready());
    assert_eq!(handle.stats().await.get("recovered").unwrap().rx_bytes, 55);
    drop(handle);
    assert!(matches!(run.await.unwrap(), Err(RunError::Stopped)));
}

#[tokio::test]
async fn closed_command_channel_returns_stopped_and_clears_readiness() {
    let (kernel, handle) = kernel(|| Ok(snapshot("peer", 3))).await;
    let ready = handle.readiness.clone();
    assert!(ready.load(Ordering::Acquire));
    let run = tokio::spawn(kernel.run());
    drop(handle);
    assert!(matches!(
        time::timeout(WAIT, run).await.unwrap().unwrap(),
        Err(RunError::Stopped)
    ));
    assert!(!ready.load(Ordering::Acquire));
}

fn assert_error_traits<E: std::error::Error + Send + Sync + 'static>() {}

#[test]
fn public_control_errors_implement_error_send_sync_static() {
    assert_error_traits::<ReloadError>();
    assert_error_traits::<SnapshotLoadError>();
    assert_error_traits::<StartError>();
    assert_error_traits::<RunError>();
}
