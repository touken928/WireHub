//! Userspace WireGuard UDP router. Policy mutations are acknowledged by the dataplane.
use std::{collections::HashMap, sync::{Arc,atomic::{AtomicBool,Ordering}}, time::{Duration, Instant}};
#[cfg(test)]
use std::net::{Ipv4Addr, SocketAddr};

#[cfg(test)]
use boringtun::{noise::{Packet, Tunn, TunnResult}, x25519::PublicKey};
use tokio::{net::UdpSocket, sync::{mpsc, oneshot, RwLock}, time};

#[cfg(test)]
use super::ipv4;

use crate::model::NetworkSnapshot;
use crate::kernel::wireguard;
#[cfg(test)]
use crate::{model::Peer, kernel::policy};

const TIMER: Duration = Duration::from_secs(1);
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
use wireguard::ANON_PARSE_COUNT;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PeerRuntimeStats { pub(crate) received_bytes:u64, pub(crate) sent_bytes:u64, pub(crate) last_handshake_unix:Option<i64>, pub(crate) last_data_unix:Option<i64> }
/// Runtime owns only transport, dataplane and per-peer accounting state.
struct RuntimeState {
    wireguard: wireguard::WireGuard,
    dataplane: crate::kernel::dataplane::DataPlane,
    stats: HashMap<String, PeerRuntimeStats>,
}
impl RuntimeState {
    fn fail_closed(&mut self) { self.wireguard.clear(); self.dataplane.clear(); self.stats.clear(); }
}

#[cfg(test)]
mod tests;

pub type SnapshotLoader = Arc<dyn Fn() -> Result<NetworkSnapshot, ()> + Send + Sync>;
pub async fn run_udp(socket: UdpSocket, loader: SnapshotLoader, hub_private: [u8; 32], commands: mpsc::Receiver<ReloadCommand>, stats: RuntimeStats, readiness: Readiness, startup: Option<oneshot::Sender<Result<(), ()>>>) -> Result<(), ()> {
    run_udp_inner(socket, loader, hub_private, commands, stats, readiness, startup).await
}

async fn publish_stats(state: &RuntimeState, target: &RuntimeStats) {
    #[cfg(test)]
    STATS_PUBLISH_COUNT.try_with(|count| { count.set(count.get()+1); STATS_PUBLISH_OBSERVER.try_with(|observer| { let _=observer.send(count.get()); }).ok(); }).ok();
    let mut published=target.write().await; published.clear();
    published.extend(state.stats.iter().map(|(id,s)| (id.clone(),(s.received_bytes,s.sent_bytes,s.last_handshake_unix,s.last_data_unix))));
}
async fn publish_packet_stats(state:&RuntimeState,target:&RuntimeStats,authenticated:bool) { if authenticated { publish_stats(state,target).await; } }
fn unix_now()->i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64 }

async fn apply_snapshot(loader:&SnapshotLoader,state:&mut RuntimeState,stats:&RuntimeStats)->Result<(),()> {
    let compiled=crate::kernel::snapshot::CompiledSnapshot::try_from(loader()?)?;
    let identities=compiled.peers.values().map(|p|wireguard::PeerIdentity{id:p.id.clone(),key:p.key,ip:p.ip}).collect();
    // Install validates all neutral identities before publishing any state.
    let retained=state.wireguard.install(identities)?;
    let mut next_stats=HashMap::new();
    for (id,peer) in &compiled.peers {
        let prior=retained.get(id).copied().unwrap_or(false).then(||state.stats.get(id).copied()).flatten();
        let (received_bytes,sent_bytes,last_handshake_unix)=prior.map(|s|(s.received_bytes,s.sent_bytes,s.last_handshake_unix)).unwrap_or_else(||compiled.initial_stats.get(id).copied().unwrap_or_default());
        next_stats.insert(id.clone(),PeerRuntimeStats{received_bytes,sent_bytes,last_handshake_unix,last_data_unix:prior.and_then(|s|s.last_data_unix)});
        let _=peer;
    }
    let old=state.dataplane.config.clone();
    let new=crate::kernel::dataplane::RoutingConfig{peers:compiled.peers.clone(),by_key:compiled.peer_by_key.clone(),forwards:compiled.forwards.clone(),hub_ip:compiled.hub_ip,by_ip:compiled.peer_by_ip.clone()};
    state.dataplane.reconcile(&old,new);
    state.stats=next_stats;
    publish_stats(state,stats).await;
    Ok(())
}

async fn run_udp_inner(socket:UdpSocket,loader:SnapshotLoader,hub_private:[u8;32],mut commands:mpsc::Receiver<ReloadCommand>,stats:RuntimeStats,readiness:Readiness,startup:Option<oneshot::Sender<Result<(),()>>>) -> Result<(),()> {
    let mut state=RuntimeState{wireguard:wireguard::WireGuard::new(hub_private),dataplane:crate::kernel::dataplane::DataPlane::default(),stats:HashMap::new()};
    let mut datagram=[0u8;65535];let mut out=vec![0u8;65535];let mut timers=time::interval(TIMER);
    readiness.set(false);
    if apply_snapshot(&loader,&mut state,&stats).await.is_err(){readiness.set(false);if let Some(ready)=startup{let _=ready.send(Err(()));}return Err(());}
    readiness.set(true);let mut retry=SnapshotRetry::new();let mut retry_at=None;
    if let Some(ready)=startup{let _=ready.send(Ok(()));}
    loop { tokio::select! { biased;
        command=commands.recv()=>{ let Some(command)=command else {readiness.set(false);return Err(())};
            let result=apply_snapshot(&loader,&mut state,&stats).await;
            if result.is_err(){eprintln!("runtime reload snapshot failed; runtime remains fail-closed");state.fail_closed();readiness.set(false);retry.succeeded();retry_at=Some(retry.failed_at(Instant::now()));}else{readiness.set(true);retry.succeeded();retry_at=None;}
            publish_stats(&state,&stats).await;let _=command.ack.send(result);
        }
        _=timers.tick(),if timers_enabled()=>{
            state.dataplane.expire(Instant::now());state.wireguard.tick(&socket,&mut out).await;drain_pending(&socket,&mut state,&mut out).await;publish_stats(&state,&stats).await;
            if retry_at.is_some_and(|at|Instant::now()>=at){match apply_snapshot(&loader,&mut state,&stats).await{Ok(())=>{readiness.set(true);retry_at=None;retry.succeeded();},Err(())=>{readiness.set(false);retry_at=Some(retry.failed_at(Instant::now()));eprintln!("runtime snapshot recovery failed; remaining fail-closed");}}}
        }
        received=socket.recv_from(&mut datagram)=>{
            let (len,endpoint)=match classify_udp_receive(received,&readiness){Ok(Some(x))=>x,Ok(None)=>continue,Err(())=>return Err(())};
            let Some(event)=state.wireguard.receive(&socket,endpoint,&datagram[..len],&mut out).await else{continue};
            let id=event.identity.id.clone();let authenticated=event.authenticated;
            if let Some(runtime)=state.stats.get_mut(&id){if event.handshake_authenticated{runtime.last_handshake_unix=Some(unix_now());}if event.data_authenticated{runtime.last_data_unix=Some(unix_now());}}
            for raw in event.packets {let Some(authenticated_peer)=state.dataplane.authenticated_peer(&id) else{continue};let Some(ingress)=state.dataplane.ingress(&authenticated_peer,&raw) else{continue};if let Some(prepared)=state.dataplane.prepare(ingress,Instant::now()){
                let outcome=state.wireguard.deliver(&socket,prepared.target_id(),prepared.bytes(),&mut out).await;
                let outcome=map_transport_outcome(outcome);
                account(&mut state.stats,state.dataplane.finish(prepared,outcome,Instant::now()));
                #[cfg(test)] if outcome==crate::kernel::dataplane::EgressOutcome::NotReady { QUEUE_OBSERVER.try_with(|observer|{let counters=state.stats.get(&id).map(|p|(p.received_bytes,p.sent_bytes)).unwrap_or_default();let _=observer.send(QueueObservation{queued:state.dataplane.test_pending_observation(),queued_bytes:state.dataplane.pending_queue_bytes(),flow_counts:state.dataplane.flow_counts(),source_counters:counters});}).ok(); }
            }}
            if authenticated{drain_pending(&socket,&mut state,&mut out).await;}publish_packet_stats(&state,&stats,authenticated).await;
            #[cfg(test)] PACKET_RESULT_OBSERVER.try_with(|observer|{let _=observer.send(authenticated);}).ok();
        }
    }}
}
fn account(stats:&mut HashMap<String,PeerRuntimeStats>,accounting:Option<crate::kernel::dataplane::DeliveryAccounting>){if let Some(a)=accounting{if let Some(s)=stats.get_mut(&a.source_id){s.received_bytes=s.received_bytes.saturating_add(a.bytes);}if let Some(s)=stats.get_mut(&a.target_id){s.sent_bytes=s.sent_bytes.saturating_add(a.bytes);}}}
fn map_transport_outcome(outcome:wireguard::TransportOutcome)->crate::kernel::dataplane::EgressOutcome{match outcome{wireguard::TransportOutcome::Delivered=>crate::kernel::dataplane::EgressOutcome::Delivered,wireguard::TransportOutcome::NotReady=>crate::kernel::dataplane::EgressOutcome::NotReady,wireguard::TransportOutcome::Failed=>crate::kernel::dataplane::EgressOutcome::Failed}}
async fn drain_pending(socket:&UdpSocket,state:&mut RuntimeState,out:&mut [u8]){let mut remaining=state.dataplane.pending_len();while remaining>0{let now=Instant::now();let Some(prepared)=state.dataplane.prepare_retry(&mut remaining,now)else{break};let result=map_transport_outcome(state.wireguard.deliver(socket,prepared.target_id(),prepared.bytes(),out).await);let accounting=state.dataplane.finish(prepared,result,Instant::now());account(&mut state.stats,accounting);}}
