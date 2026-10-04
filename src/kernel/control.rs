use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::{mpsc, oneshot, RwLock};

const RELOAD_SEND_TIMEOUT: Duration = Duration::from_secs(3);
const RELOAD_ACK_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) type SharedStats = Arc<RwLock<Arc<HashMap<String, PeerRuntimeStats>>>>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerRuntimeStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub last_handshake_unix: Option<i64>,
    pub last_data_unix: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ReloadError {
    #[error("kernel is stopped")]
    Stopped,
    #[error("kernel reload timed out")]
    Timeout,
    #[error("kernel rejected the snapshot reload")]
    Rejected,
}

pub(super) struct ReloadCommand {
    pub(super) minimum_revision: i64,
    pub(super) ack: oneshot::Sender<Result<i64, ReloadError>>,
}

pub(crate) struct ReloadPermit<'a> {
    permit: mpsc::Permit<'a, ReloadCommand>,
}

pub(crate) struct ReloadAck(oneshot::Receiver<Result<i64, ReloadError>>);

impl ReloadPermit<'_> {
    pub(crate) fn send(self) -> ReloadAck {
        self.send_at(0)
    }
    pub(crate) fn send_at(self, minimum_revision: i64) -> ReloadAck {
        let (ack, wait) = oneshot::channel();
        self.permit.send(ReloadCommand {
            minimum_revision,
            ack,
        });
        ReloadAck(wait)
    }
}

impl ReloadAck {
    pub(crate) async fn wait(self) -> Result<(), ReloadError> {
        self.wait_revision().await.map(|_| ())
    }
    pub(crate) async fn wait_revision(self) -> Result<i64, ReloadError> {
        match tokio::time::timeout(RELOAD_ACK_TIMEOUT, self.0).await {
            Err(_) => Err(ReloadError::Timeout),
            Ok(Err(_)) => Err(ReloadError::Stopped),
            Ok(Ok(result)) => result,
        }
    }
}

#[derive(Clone)]
pub struct KernelHandle {
    pub(super) commands: mpsc::Sender<ReloadCommand>,
    pub(super) readiness: Arc<std::sync::atomic::AtomicBool>,
    pub(super) stats: SharedStats,
    pub(super) status: Arc<RwLock<ActivationStatus>>,
}

#[derive(Clone, Debug, Default)]
pub struct ActivationStatus {
    pub applied_revision: Option<i64>,
    pub last_error: Option<String>,
}

impl KernelHandle {
    pub fn is_ready(&self) -> bool {
        self.readiness.load(std::sync::atomic::Ordering::Acquire)
    }
    pub async fn activation(&self) -> ActivationStatus {
        self.status.read().await.clone()
    }

    pub(crate) async fn reserve_reload(&self) -> Result<ReloadPermit<'_>, ReloadError> {
        let permit = tokio::time::timeout(RELOAD_SEND_TIMEOUT, self.commands.reserve())
            .await
            .map_err(|_| ReloadError::Timeout)?
            .map_err(|_| ReloadError::Stopped)?;
        Ok(ReloadPermit { permit })
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "retained kernel control API; management writes reserve before mutating"
        )
    )]
    pub async fn reload(&self) -> Result<(), ReloadError> {
        self.reserve_reload().await?.send().wait().await
    }

    pub async fn stats(&self) -> Arc<HashMap<String, PeerRuntimeStats>> {
        let stats = self.stats.read().await.clone();
        if self.is_ready() {
            stats
        } else {
            Arc::default()
        }
    }
}

#[cfg(test)]
pub(crate) struct TestReceiver(mpsc::Receiver<ReloadCommand>);

#[cfg(test)]
pub(crate) struct TestReloadCommand(ReloadCommand);

#[cfg(test)]
impl TestReloadCommand {
    pub(crate) fn respond(self, result: Result<(), ReloadError>) {
        let _ = self.0.ack.send(result.map(|_| self.0.minimum_revision));
    }
}

#[cfg(test)]
impl TestReceiver {
    pub(crate) async fn recv(&mut self) -> Option<TestReloadCommand> {
        self.0.recv().await.map(TestReloadCommand)
    }
}

#[cfg(test)]
pub(crate) fn test_harness() -> (KernelHandle, TestReceiver) {
    let (commands, receiver) = mpsc::channel(16);
    (
        KernelHandle {
            commands,
            readiness: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(Arc::default())),
            status: Arc::new(RwLock::new(ActivationStatus::default())),
        },
        TestReceiver(receiver),
    )
}

#[cfg(test)]
mod tests;
