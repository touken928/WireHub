use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::{mpsc, oneshot, RwLock};

const RELOAD_SEND_TIMEOUT: Duration = Duration::from_secs(3);
const RELOAD_ACK_TIMEOUT: Duration = Duration::from_secs(3);

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
    pub(super) ack: oneshot::Sender<Result<(), ReloadError>>,
}

#[derive(Clone)]
pub struct KernelHandle {
    pub(super) commands: mpsc::Sender<ReloadCommand>,
    pub(super) readiness: Arc<std::sync::atomic::AtomicBool>,
    pub(super) stats: Arc<RwLock<HashMap<String, PeerRuntimeStats>>>,
}

impl KernelHandle {
    pub fn is_ready(&self) -> bool {
        self.readiness.load(std::sync::atomic::Ordering::Acquire)
    }

    pub async fn reload(&self) -> Result<(), ReloadError> {
        let (ack, wait) = oneshot::channel();
        tokio::time::timeout(
            RELOAD_SEND_TIMEOUT,
            self.commands.send(ReloadCommand { ack }),
        )
        .await
        .map_err(|_| ReloadError::Timeout)?
        .map_err(|_| ReloadError::Stopped)?;
        match tokio::time::timeout(RELOAD_ACK_TIMEOUT, wait).await {
            Err(_) => Err(ReloadError::Timeout),
            Ok(Err(_)) => Err(ReloadError::Stopped),
            Ok(Ok(result)) => result,
        }
    }

    pub async fn stats(&self) -> Arc<HashMap<String, PeerRuntimeStats>> {
        Arc::new(self.stats.read().await.clone())
    }
}

#[cfg(test)]
pub(crate) struct TestReceiver(mpsc::Receiver<ReloadCommand>);

#[cfg(test)]
pub(crate) struct TestReloadCommand(ReloadCommand);

#[cfg(test)]
impl TestReloadCommand {
    pub(crate) fn respond(self, result: Result<(), ReloadError>) {
        let _ = self.0.ack.send(result);
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
            stats: Arc::new(RwLock::new(HashMap::new())),
        },
        TestReceiver(receiver),
    )
}

#[cfg(test)]
mod tests;
