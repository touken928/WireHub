//! Userspace WireGuard UDP router. Policy mutations are acknowledged by the dataplane.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tokio::{
    net::UdpSocket,
    sync::{mpsc, RwLock},
    time,
};

use super::{
    control::{ActivationStatus, KernelHandle, PeerRuntimeStats, ReloadCommand, ReloadError},
    wireguard,
};

const TIMER: Duration = Duration::from_secs(1);
const RETRY_INITIAL: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("runtime snapshot could not be loaded or compiled")]
pub struct SnapshotLoadError;
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("initial snapshot could not be loaded or compiled: {0}")]
    Snapshot(#[source] SnapshotLoadError),
}
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("UDP socket failed: {0}")]
    Socket(#[from] std::io::Error),
    #[error("kernel command channel stopped")]
    Stopped,
}

#[derive(Debug)]
struct SnapshotRetry {
    delay: Duration,
}
impl SnapshotRetry {
    fn new() -> Self {
        Self {
            delay: RETRY_INITIAL,
        }
    }
    fn failed_at(&mut self, now: Instant) -> Instant {
        let retry_at = now + self.delay;
        self.delay = self.delay.saturating_mul(2).min(RETRY_MAX);
        retry_at
    }
    fn succeeded(&mut self) {
        self.delay = RETRY_INITIAL;
    }
}

fn recoverable_udp_error(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::AddrNotAvailable
    )
}

fn classify_udp_receive<T>(
    received: std::io::Result<T>,
    readiness: &AtomicBool,
) -> Result<Option<T>, std::io::Error> {
    match received {
        Ok(datagram) => Ok(Some(datagram)),
        Err(error) if recoverable_udp_error(error.kind()) => {
            eprintln!("recoverable UDP receive error: {:?}", error.kind());
            Ok(None)
        }
        Err(error) => {
            readiness.store(false, Ordering::Release);
            eprintln!("fatal UDP receive error: {:?}", error.kind());
            Err(error)
        }
    }
}

#[cfg(test)]
use super::ipv4;
#[cfg(test)]
use super::wireguard::ANON_PARSE_COUNT;
#[cfg(test)]
use crate::{kernel::policy, model::Peer};
#[cfg(test)]
use boringtun::{
    noise::{Packet, Tunn, TunnResult},
    x25519::PublicKey,
};
#[cfg(test)]
use std::net::Ipv4Addr;
#[cfg(test)]
use std::net::SocketAddr;

#[cfg(test)]
tokio::task_local! { static STATS_PUBLISH_COUNT: std::cell::Cell<usize>; }
#[cfg(test)]
tokio::task_local! { static STATS_PUBLISH_OBSERVER: mpsc::UnboundedSender<usize>; }
#[cfg(test)]
tokio::task_local! { static PACKET_RESULT_OBSERVER: mpsc::UnboundedSender<bool>; }
#[cfg(test)]
tokio::task_local! { static DISABLE_TIMERS_FOR_TEST: bool; }
#[cfg(test)]
fn timers_enabled() -> bool {
    !DISABLE_TIMERS_FOR_TEST
        .try_with(|disabled| *disabled)
        .unwrap_or(false)
}
#[cfg(not(test))]
fn timers_enabled() -> bool {
    true
}
#[cfg(test)]
#[derive(Debug)]
struct QueueObservation {
    queued: Vec<crate::kernel::dataplane::PendingObservation>,
    queued_bytes: usize,
    flow_counts: (usize, usize),
    source_counters: (u64, u64),
}
#[cfg(test)]
tokio::task_local! { static QUEUE_OBSERVER: mpsc::UnboundedSender<QueueObservation>; }

struct RuntimeState {
    wireguard: wireguard::WireGuard,
    dataplane: crate::kernel::dataplane::DataPlane,
    stats: HashMap<String, PeerRuntimeStats>,
}
impl RuntimeState {
    fn fail_closed(&mut self) {
        self.wireguard.clear();
        self.dataplane.clear();
        self.stats.clear();
    }
}

pub struct Kernel {
    socket: UdpSocket,
    loader: Box<dyn FnMut() -> Result<crate::kernel::CompiledSnapshot, SnapshotLoadError> + Send>,
    commands: mpsc::Receiver<ReloadCommand>,
    stats: super::control::SharedStats,
    readiness: Arc<AtomicBool>,
    status: Arc<RwLock<ActivationStatus>>,
    state: RuntimeState,
    _lifecycle: LifecycleGuard,
}

struct LifecycleGuard {
    readiness: Arc<AtomicBool>,
    stats: super::control::SharedStats,
}
impl Drop for LifecycleGuard {
    fn drop(&mut self) {
        self.readiness.store(false, Ordering::Release);
        if let Ok(mut stats) = self.stats.try_write() {
            *stats = Arc::default();
        }
    }
}

impl Kernel {
    pub async fn initialize<F>(
        socket: UdpSocket,
        hub_private: [u8; 32],
        mut load: F,
    ) -> Result<(Self, KernelHandle), StartError>
    where
        F: FnMut() -> Result<crate::kernel::CompiledSnapshot, SnapshotLoadError> + Send + 'static,
    {
        let initial = load().map_err(StartError::Snapshot)?;
        let readiness = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(RwLock::new(Arc::default()));
        let status = Arc::new(RwLock::new(ActivationStatus {
            applied_revision: Some(initial.revision),
            last_error: None,
        }));
        let (commands, receiver) = mpsc::channel(16);
        let handle = KernelHandle {
            commands,
            readiness: readiness.clone(),
            stats: stats.clone(),
            status: status.clone(),
        };
        let mut state = RuntimeState {
            wireguard: wireguard::WireGuard::new(hub_private),
            dataplane: crate::kernel::dataplane::DataPlane::default(),
            stats: HashMap::new(),
        };
        install_snapshot(initial, &mut state)
            .map_err(|_| StartError::Snapshot(SnapshotLoadError))?;
        publish_stats(&state, &stats).await;
        readiness.store(true, Ordering::Release);
        Ok((
            Self {
                socket,
                loader: Box::new(load),
                commands: receiver,
                stats: stats.clone(),
                readiness: readiness.clone(),
                status,
                state,
                _lifecycle: LifecycleGuard { readiness, stats },
            },
            handle,
        ))
    }

    pub async fn run(mut self) -> Result<(), RunError> {
        let mut datagram = [0u8; 65535];
        let mut out = vec![0u8; 65535];
        let mut timers = time::interval_at(time::Instant::now() + TIMER, TIMER);
        timers.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut retry = SnapshotRetry::new();
        let mut retry_at = None;
        let mut minimum_revision = self.status.read().await.applied_revision.unwrap_or(0);
        loop {
            tokio::select! { biased;
                command=self.commands.recv()=>{
                    let Some(command)=command else{self.readiness.store(false,Ordering::Release);return Err(RunError::Stopped)};
                    minimum_revision = minimum_revision.max(command.minimum_revision);
                    let result=(self.loader)().map_err(|_|()).and_then(|snapshot| {
                        let revision = snapshot.revision;
                        if revision < minimum_revision { return Err(()); }
                        install_snapshot(snapshot,&mut self.state).map(|_| revision)
                    });
                    if result.is_err(){eprintln!("runtime reload snapshot failed; runtime remains fail-closed");self.readiness.store(false,Ordering::Release);self.state.fail_closed();retry.succeeded();retry_at=Some(retry.failed_at(Instant::now()));}
                    else{retry.succeeded();retry_at=None;}
                    publish_stats(&self.state,&self.stats).await;
                    match result {
                        Ok(revision) => { minimum_revision = revision; *self.status.write().await = ActivationStatus { applied_revision: Some(revision), last_error: None }; self.readiness.store(true,Ordering::Release); }
                        Err(()) => { self.status.write().await.last_error = Some("snapshot_rejected".into()); }
                    }
                    let _=command.ack.send(result.map_err(|_|ReloadError::Rejected));
                }
                _=timers.tick(),if timers_enabled()=>{
                    self.state.dataplane.expire(Instant::now());self.state.wireguard.tick(&self.socket,&mut out).await;drain_pending(&self.socket,&mut self.state,&mut out).await;publish_stats(&self.state,&self.stats).await;
                    if retry_at.is_some_and(|at|Instant::now()>=at){match (self.loader)().map_err(|_|()).and_then(|snapshot|{let revision=snapshot.revision;if revision<minimum_revision{return Err(());}install_snapshot(snapshot,&mut self.state).map(|_|revision)}){Ok(revision)=>{minimum_revision=revision;publish_stats(&self.state,&self.stats).await;*self.status.write().await=ActivationStatus{applied_revision:Some(revision),last_error:None};self.readiness.store(true,Ordering::Release);retry_at=None;retry.succeeded();},Err(())=>{self.readiness.store(false,Ordering::Release);retry_at=Some(retry.failed_at(Instant::now()));eprintln!("runtime snapshot recovery failed; remaining fail-closed");}}}
                }
                received=self.socket.recv_from(&mut datagram)=>{
                     let (len,endpoint)=match classify_udp_receive(received,&self.readiness){Ok(Some(x))=>x,Ok(None)=>continue,Err(error)=>return Err(RunError::Socket(error))};
                    let Some(event)=self.state.wireguard.receive(&self.socket,endpoint,&datagram[..len],&mut out).await else{continue};
                    let id=event.identity.id.clone();let authenticated=event.authenticated;
                    if let Some(runtime)=self.state.stats.get_mut(&id){
                        if event.handshake_authenticated{runtime.last_handshake_unix=Some(unix_now());}
                        if event.data_authenticated{runtime.last_data_unix=Some(unix_now());}
                    }
                    for raw in event.packets {let Some(authenticated_peer)=self.state.dataplane.authenticated_peer(&id)else{continue};let Some(ingress)=self.state.dataplane.ingress(&authenticated_peer,&raw)else{continue};if let Some(prepared)=self.state.dataplane.prepare(ingress,Instant::now()){
                        let outcome=map_transport_outcome(self.state.wireguard.deliver(&self.socket,prepared.target_id(),prepared.bytes(),&mut out).await);
                        account(&mut self.state.stats,self.state.dataplane.finish(prepared,outcome,Instant::now()));
                        #[cfg(test)] if outcome==crate::kernel::dataplane::EgressOutcome::NotReady{QUEUE_OBSERVER.try_with(|observer|{let counters=self.state.stats.get(&id).map(|p|(p.rx_bytes,p.tx_bytes)).unwrap_or_default();let _=observer.send(QueueObservation{queued:self.state.dataplane.test_pending_observation(),queued_bytes:self.state.dataplane.pending_queue_bytes(),flow_counts:self.state.dataplane.flow_counts(),source_counters:counters});}).ok();}
                    }}
                    if authenticated{drain_pending(&self.socket,&mut self.state,&mut out).await;}
                    #[cfg(test)] PACKET_RESULT_OBSERVER.try_with(|observer|{let _=observer.send(authenticated);}).ok();
                }
            }
        }
    }
}

fn install_snapshot(
    compiled: crate::kernel::CompiledSnapshot,
    state: &mut RuntimeState,
) -> Result<(), ()> {
    let identities = compiled
        .peers
        .values()
        .map(|p| wireguard::PeerIdentity {
            id: p.id.clone(),
            key: p.key,
            ip: p.ip,
        })
        .collect();
    let retained = state.wireguard.install(identities)?;
    let mut next_stats = HashMap::new();
    for id in compiled.peers.keys() {
        let prior = retained
            .get(id)
            .copied()
            .unwrap_or(false)
            .then(|| state.stats.get(id).copied())
            .flatten();
        let (rx_bytes, tx_bytes, last_handshake_unix) = prior
            .map(|s| (s.rx_bytes, s.tx_bytes, s.last_handshake_unix))
            .unwrap_or_else(|| compiled.initial_stats.get(id).copied().unwrap_or_default());
        next_stats.insert(
            id.clone(),
            PeerRuntimeStats {
                rx_bytes,
                tx_bytes,
                last_handshake_unix,
                last_data_unix: prior.and_then(|s| s.last_data_unix),
            },
        );
    }
    let old = state.dataplane.config.clone();
    let new = crate::kernel::dataplane::RoutingConfig {
        peers: compiled.peers.clone(),
        forwards: compiled.forwards.clone(),
        forward_index: compiled.forward_index.clone(),
        hub_ip: compiled.hub_ip,
        by_ip: compiled.peer_by_ip.clone(),
    };
    state.dataplane.reconcile(&old, new);
    state.stats = next_stats;
    Ok(())
}

async fn publish_stats(state: &RuntimeState, target: &super::control::SharedStats) {
    #[cfg(test)]
    STATS_PUBLISH_COUNT
        .try_with(|count| {
            count.set(count.get() + 1);
            STATS_PUBLISH_OBSERVER
                .try_with(|observer| {
                    let _ = observer.send(count.get());
                })
                .ok();
        })
        .ok();
    *target.write().await = Arc::new(state.stats.clone());
}
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn account(
    stats: &mut HashMap<String, PeerRuntimeStats>,
    accounting: Option<crate::kernel::dataplane::DeliveryAccounting>,
) {
    if let Some(a) = accounting {
        if let Some(s) = stats.get_mut(&a.source_id) {
            s.rx_bytes = s.rx_bytes.saturating_add(a.bytes);
        }
        if let Some(s) = stats.get_mut(&a.target_id) {
            s.tx_bytes = s.tx_bytes.saturating_add(a.bytes);
        }
    }
}
fn map_transport_outcome(
    outcome: wireguard::TransportOutcome,
) -> crate::kernel::dataplane::EgressOutcome {
    match outcome {
        wireguard::TransportOutcome::Delivered => {
            crate::kernel::dataplane::EgressOutcome::Delivered
        }
        wireguard::TransportOutcome::NotReady => crate::kernel::dataplane::EgressOutcome::NotReady,
        wireguard::TransportOutcome::Failed => crate::kernel::dataplane::EgressOutcome::Failed,
    }
}
async fn drain_pending(socket: &UdpSocket, state: &mut RuntimeState, out: &mut [u8]) {
    let mut remaining = state.dataplane.pending_len();
    while remaining > 0 {
        let now = Instant::now();
        let Some(prepared) = state.dataplane.prepare_retry(&mut remaining, now) else {
            break;
        };
        let result = map_transport_outcome(
            state
                .wireguard
                .deliver(socket, prepared.target_id(), prepared.bytes(), out)
                .await,
        );
        let accounting = state.dataplane.finish(prepared, result, Instant::now());
        account(&mut state.stats, accounting);
    }
}

#[cfg(test)]
mod tests;
