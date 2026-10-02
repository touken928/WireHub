//! Userspace WireGuard UDP router. Policy mutations are acknowledged by the dataplane.
use std::{collections::{HashMap, VecDeque}, net::{Ipv4Addr,SocketAddr}, sync::{Arc,atomic::{AtomicBool,Ordering}}, time::{Duration, Instant}};

use boringtun::{noise::{handshake::parse_handshake_anon, rate_limiter::RateLimiter, Packet, Tunn, TunnResult}, x25519::{PublicKey, StaticSecret}};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use tokio::{net::UdpSocket, sync::{mpsc, oneshot, RwLock}, time};

#[path = "ipv4.rs"]
pub(crate) mod ipv4;

use crate::{model::{Forward, Group, Peer}, flows::{Flows, Reservation}, network::Subnet24, storage::Store, policy};

const TIMER: Duration = Duration::from_secs(1);
const PENDING_LIMIT: usize = 256;
const PENDING_BYTES: usize = 1024 * 1024;
const PENDING_TTL: Duration = Duration::from_secs(3);
const RETRY_INITIAL: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct SnapshotRetry { delay: Duration }
impl SnapshotRetry {
    fn new() -> Self { Self { delay: RETRY_INITIAL } }
    fn failed_at(&mut self, now: Instant) -> Instant {
        let retry_at = now + self.delay;
        self.delay = self.delay.saturating_mul(2).min(RETRY_MAX);
        retry_at
    }
    fn succeeded(&mut self) { self.delay = RETRY_INITIAL; }
}

fn recoverable_udp_error(kind: std::io::ErrorKind) -> bool {
    matches!(kind, std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable)
}

fn classify_udp_receive<T>(received: std::io::Result<T>, readiness: &Readiness) -> Result<Option<T>, ()> {
    match received {
        Ok(datagram) => Ok(Some(datagram)),
        Err(error) if recoverable_udp_error(error.kind()) => {
            eprintln!("recoverable UDP receive error: {:?}", error.kind());
            Ok(None)
        }
        Err(error) => {
            readiness.set(false);
            eprintln!("fatal UDP receive error: {:?}", error.kind());
            Err(())
        }
    }
}

#[cfg(test)]
thread_local! { static ANON_PARSE_COUNT: AtomicUsize = AtomicUsize::new(0); }

#[cfg(test)]
tokio::task_local! { static STATS_PUBLISH_COUNT: std::cell::Cell<usize>; }
#[cfg(test)]
tokio::task_local! { static STATS_PUBLISH_OBSERVER: mpsc::UnboundedSender<usize>; }
#[cfg(test)]
tokio::task_local! { static PACKET_RESULT_OBSERVER: mpsc::UnboundedSender<bool>; }
#[cfg(test)]
tokio::task_local! { static DISABLE_TIMERS_FOR_TEST: bool; }

#[cfg(test)]
fn timers_enabled() -> bool { !DISABLE_TIMERS_FOR_TEST.try_with(|disabled| *disabled).unwrap_or(false) }
#[cfg(not(test))]
fn timers_enabled() -> bool { true }

#[cfg(test)]
#[derive(Debug)]
struct QueueObservation {
    queued: Vec<(String, String, Option<String>, Ipv4Addr, Ipv4Addr, Ipv4Addr, u16, u8)>,
    queued_bytes: usize,
    flow_counts: (usize, usize),
    source_counters: (u64, u64),
}

#[cfg(test)]
tokio::task_local! { static QUEUE_OBSERVER: mpsc::UnboundedSender<QueueObservation>; }

pub type RuntimeStats = Arc<RwLock<HashMap<String, (u64, u64, Option<i64>, Option<i64>)>>>;
#[derive(Clone, Default)]
pub struct Readiness(Arc<AtomicBool>);
impl Readiness {
    pub fn is_ready(&self)->bool { self.0.load(Ordering::Acquire) }
    pub fn set(&self, ready:bool) { self.0.store(ready,Ordering::Release); }
}
pub struct ReloadCommand { pub ack: oneshot::Sender<Result<(), ()>> }

pub(crate) struct RuntimePeer {
    pub(crate) peer: Peer,
    pub(crate) group: Option<Group>,
    pub(crate) tunnel: Tunn,
    pub(crate) endpoint: Option<SocketAddr>,
    pub(crate) last_data_unix: Option<i64>,
    receiver_index: u32,
}

/// Mutable routing state installed by a single validated snapshot.
#[derive(Default)]
struct RouterState {
    peers: HashMap<String, RuntimePeer>,
    indexes: HashMap<u32, String>,
    next_index: u32,
    forwards: Vec<Forward>,
    hub_ip: Option<Ipv4Addr>,
    flows: Flows,
    pending: VecDeque<delivery::PendingDelivery>,
    pending_bytes: usize,
}
impl RouterState {
    /// Drop installed dataplane state after a failed reload without reusing
    /// receiver indexes allocated during this process.
    fn fail_closed(&mut self) {
        self.peers.clear();
        self.indexes.clear();
        self.forwards.clear();
        self.hub_ip = None;
        self.flows.clear();
        self.pending.clear();
        self.pending_bytes = 0;
    }
}


mod runtime;
mod snapshot;
mod delivery;

#[cfg(test)]
#[path = "transport/tests.rs"]
mod tests;

pub async fn run_udp(socket: UdpSocket, store: Arc<Store>, hub_private: [u8; 32], commands: mpsc::Receiver<ReloadCommand>, stats: RuntimeStats, readiness: Readiness, startup: Option<oneshot::Sender<Result<(), ()>>>) -> Result<(), ()> {
    runtime::run_udp(socket, store, hub_private, commands, stats, readiness, startup).await
}
