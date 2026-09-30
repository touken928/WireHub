//! Userspace WireGuard UDP router. Policy mutations are acknowledged by the dataplane.
use std::{collections::{HashMap, VecDeque}, net::{Ipv4Addr,SocketAddr}, sync::Arc, time::{Duration, Instant}};

use boringtun::{noise::{handshake::parse_handshake_anon, Packet, Tunn, TunnResult}, x25519::{PublicKey, StaticSecret}};
use tokio::{net::UdpSocket, sync::{mpsc, oneshot, RwLock}, time};

#[path = "ipv4.rs"]
pub(crate) mod ipv4;

use crate::{model::{Forward, Group, Peer}, flows::{Flows, Reservation}, network::Subnet24, storage::Store, policy};

const TIMER: Duration = Duration::from_secs(1);
const PENDING_LIMIT: usize = 256;
const PENDING_BYTES: usize = 1024 * 1024;
const PENDING_TTL: Duration = Duration::from_secs(3);

pub type RuntimeStats = Arc<RwLock<HashMap<String, (u64, u64, Option<i64>, Option<i64>)>>>;
pub struct ReloadCommand { pub ack: oneshot::Sender<Result<(), ()>> }

struct RuntimePeer {
    peer: Peer,
    group: Option<Group>,
    tunnel: Tunn,
    endpoint: Option<SocketAddr>,
    last_data_unix: Option<i64>,
}

/// Run the WireGuard router on an already-bound UDP socket. `hub_private` is
/// the hub's persisted static private key; it is never written by this module.
pub async fn run_udp(socket: UdpSocket, store: Arc<Store>, hub_private: [u8; 32], mut commands: mpsc::Receiver<ReloadCommand>, stats: RuntimeStats, startup: Option<oneshot::Sender<Result<(), ()>>>) {
    let hub_secret = StaticSecret::from(hub_private);
    let hub_public = PublicKey::from(&hub_secret);
    let mut peers: HashMap<String, RuntimePeer> = HashMap::new();
    let mut forwards: Vec<Forward> = Vec::new();
    let mut hub_ip: Option<Ipv4Addr> = None;
    let mut nat = Flows::default();
    let mut queued_deliveries = VecDeque::<PendingDelivery>::new();
    let mut pending_bytes = 0usize;
    let mut indexes: HashMap<u32, String> = HashMap::new();
    let mut datagram = [0u8; 65535];
    let mut out = vec![0u8; 65535];
    let mut timers = time::interval(TIMER);
    let mut next_index = 1u32;

    // Persisted peers must be installed before the listener is reported ready.
    let startup_result = async {
        let snapshot = store.runtime_snapshot().map_err(|_| ())?;
        validate_persisted_addresses(&snapshot)?;
        hub_ip = snapshot.settings.as_ref().map(|settings| Subnet24::parse(&settings.subnet).map_err(|_| ())?.hub_ip().parse::<Ipv4Addr>().map_err(|_| ())).transpose()?;
        forwards = snapshot.forwards;
        if let Some(hub) = hub_ip { nat = Flows::new(hub, &forwards); }
        load_peers(snapshot.groups, snapshot.peers, hub_private, &mut peers, &mut indexes, &mut next_index, &stats).await
    }.await;
    if let Some(ready) = startup { let _ = ready.send(startup_result); }
    if startup_result.is_err() { return; }

    loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else { break };
                queued_deliveries.clear(); pending_bytes = 0;
                // Clear first: any database/validation failure leaves no stale policy active.
                let old = std::mem::take(&mut peers);
                indexes.clear();
                let result = store.runtime_snapshot().map_err(|_| ()).and_then(|snapshot| {
                    validate_persisted_addresses(&snapshot)?;
                    let updated_hub_ip = snapshot.settings.as_ref().map(|settings| Subnet24::parse(&settings.subnet).map_err(|_| ())?.hub_ip().parse::<Ipv4Addr>().map_err(|_| ())).transpose()?;
                    install_peers(&snapshot.groups, snapshot.peers, hub_private, &mut peers, &mut indexes, &mut next_index, old)?;
                    forwards = snapshot.forwards;
                    hub_ip = updated_hub_ip;
                    Ok(())
                });
                nat.clear();
                if result.is_ok() { if let Some(hub) = hub_ip { nat = Flows::new(hub, &forwards); } }
                if result.is_err() { peers.clear(); indexes.clear(); }
                publish_stats(&peers, &stats).await;
                let _ = command.ack.send(result);
            }
            _ = timers.tick() => {
                nat.expire(Instant::now());
                for runtime in peers.values_mut() {
                    let result = runtime.tunnel.update_timers(&mut out);
                    if let (TunnResult::WriteToNetwork(packet), Some(endpoint)) = (result, runtime.endpoint) { let _ = socket.send_to(packet, endpoint).await; }
                }
                drain_pending(&socket, &mut queued_deliveries, &mut pending_bytes, &mut peers, &forwards, hub_ip, &mut nat, &mut out).await;
                publish_stats(&peers, &stats).await;
            }
            received = socket.recv_from(&mut datagram) => {
                let Ok((len, endpoint)) = received else { continue };
                let bytes = &datagram[..len];
                let parsed = Tunn::parse_incoming_packet(bytes);
                let packet_kind = parsed.as_ref().ok().map(|packet| match packet { Packet::HandshakeInit(_) => 1, Packet::HandshakeResponse(_) => 2, Packet::PacketCookieReply(_) => 3, Packet::PacketData(_) => 4 });
                let peer_id = match parsed {
                    Ok(Packet::HandshakeInit(init)) => {
                        // This lookup is only a demux hint. Tunn must verify the
                        // full initiation before any peer endpoint is changed.
                        parse_handshake_anon(&hub_secret, &hub_public, &init).ok()
                            .and_then(|h| peers.iter().find(|(_, p)| decode_public_key(&p.peer.public_key).ok().as_ref() == Some(&h.peer_static_public)).map(|(id, _)| id.clone()))
                    }
                    Ok(Packet::HandshakeResponse(resp)) => indexes.get(&(resp.receiver_idx & 0xffff_ff00)).cloned(),
                    Ok(Packet::PacketCookieReply(cookie)) => indexes.get(&(cookie.receiver_idx & 0xffff_ff00)).cloned(),
                    Ok(Packet::PacketData(data)) => indexes.get(&(data.receiver_idx & 0xffff_ff00)).cloned(),
                    Err(_) => None,
                };
                let Some(id) = peer_id else { continue };
                let Some(runtime) = peers.get_mut(&id) else { continue };
                let result = runtime.tunnel.decapsulate(Some(endpoint.ip()), bytes, &mut out);
                let mut authenticated = false;
                let mut handshake_authenticated = false;
                let mut data_authenticated = false;
                let mut packet_queue = Vec::new();
                let initial_result = &result;
                let accepted_handshake_response = packet_kind == Some(2)
                    && matches!(initial_result, TunnResult::WriteToNetwork(packet)
                        if matches!(Tunn::parse_incoming_packet(packet), Ok(Packet::PacketData(_))));
                let mut pending = result;
                let mut first_result = true;
                loop {
                    match pending {
                        TunnResult::WriteToNetwork(packet) => {
                            let payload = packet.to_vec();
                            // A cookie response is not authentication. An
                            // authenticated initiation yields a handshake response.
                            if packet_kind == Some(1) && payload.len() >= 4 && u32::from_le_bytes(payload[..4].try_into().unwrap()) == 2 { authenticated = true; handshake_authenticated = true; }
                            // BoringTun accepts an authenticated handshake response
                            // by immediately emitting an encrypted transport keepalive.
                            // Require both the incoming response context and that
                            // exact initial output type; arbitrary network output is
                            // not proof that a response was accepted.
                            if first_result && accepted_handshake_response {
                                authenticated = true;
                                handshake_authenticated = true;
                            }
                            first_result = false;
                            let _ = socket.send_to(&payload, endpoint).await;
                            pending = runtime.tunnel.decapsulate(None, &[], &mut out);
                        }
                        TunnResult::WriteToTunnelV4(packet, _) => {
                            first_result = false;
                            authenticated = true;
                            data_authenticated = true;
                            if let Some(packet) = ipv4::validate(packet, &runtime.peer, runtime.group.as_ref()) {
                                packet_queue.push(packet);
                            }
                            pending = runtime.tunnel.decapsulate(None, &[], &mut out);
                        }
                        TunnResult::WriteToTunnelV6(_, _) => { first_result = false; data_authenticated = true; pending = runtime.tunnel.decapsulate(None, &[], &mut out); }
                        // BoringTun 0.7.1 returns Done for an authenticated
                        // empty transport packet (keepalive), but also for a
                        // cookie reply. Never infer response authentication
                        // from Done; it is recorded only for the expected
                        // transport keepalive emitted after response acceptance.
                        TunnResult::Done => { if packet_kind == Some(4) { authenticated = true; data_authenticated = true; } break; }
                        TunnResult::Err(_) => break,
                    }
                }
                if authenticated { runtime.endpoint = Some(endpoint); }
                if handshake_authenticated { runtime.peer.last_handshake_unix = Some(unix_now()); }
                if data_authenticated { runtime.last_data_unix = Some(unix_now()); }
                let _ = runtime;
                for packet in packet_queue {
                    let now = std::time::Instant::now();
                    // `id` came from the authenticated tunnel receiver index. Keep
                    // that identity; never infer a source peer from packet IP.
                    let Some(source) = peers.get(&id).map(|p| p.peer.clone()) else { continue };
                    let Some(source_group) = peers.get(&id).and_then(|p| p.group.clone()) else { continue };
                    let (plan, was_reply) = resolve_packet(&packet, &source, &source_group, &id, &peers, &forwards, hub_ip, &mut nat, now);
                    if let Some(mut plan) = plan {
                        let outcome = deliver_plan(&socket, &mut peers, &plan, &mut out).await;
                        complete_delivery(&mut nat, &mut peers, &mut plan, outcome);
                        if outcome == EgressOutcome::NotReady {
                            enqueue_pending(&mut queued_deliveries, &mut pending_bytes, PendingDelivery { source_id: id.clone(), packet: packet.clone(), reply_only: was_reply, deadline: Instant::now() + PENDING_TTL });
                        }
                    }
                }
                if authenticated { drain_pending(&socket, &mut queued_deliveries, &mut pending_bytes, &mut peers, &forwards, hub_ip, &mut nat, &mut out).await; }
                publish_stats(&peers, &stats).await;
            }
        }
    }
}

struct DeliveryPlan { source_id: String, target_id: String, bytes: Vec<u8>, reservation: Option<Reservation>, udp: bool }

struct PendingDelivery { source_id: String, packet: ipv4::ValidatedPacket, reply_only: bool, deadline: Instant }
impl PendingDelivery { fn expired_at(&self, now: Instant) -> bool { self.deadline <= now } }

fn enqueue_pending(queue: &mut VecDeque<PendingDelivery>, bytes: &mut usize, delivery: PendingDelivery) {
    let size = delivery.packet.bytes().len();
    if queue.len() >= PENDING_LIMIT || bytes.saturating_add(size) > PENDING_BYTES { return; }
    *bytes += size;
    queue.push_back(delivery);
}

fn resolve_packet(packet: &ipv4::ValidatedPacket, source: &Peer, source_group: &Group, source_id: &str, peers: &HashMap<String, RuntimePeer>, forwards: &[Forward], hub_ip: Option<Ipv4Addr>, nat: &mut Flows, now: Instant) -> (Option<DeliveryPlan>, bool) {
    if let Some((target_id, bytes, reservation)) = nat.lookup_reply(packet, source, now) {
        return (Some(DeliveryPlan { source_id: source_id.into(), target_id, bytes, reservation: Some(reservation), udp: packet.protocol() == 17 }), true);
    }
    let mut terminal = false;
    let mut plan = None;
    if packet.dst() == hub_ip.unwrap_or(Ipv4Addr::UNSPECIFIED) {
        for forward in forwards {
            let Some(target) = peers.get(&forward.target_peer_id) else { continue };
            if !policy::forward_allowed(forward, &source.group_id, Some(source_group), &target.peer.group_id) { continue; }
            if let Some((bytes, reservation)) = nat.prepare_forward_packet(packet, source, forward, &target.peer, now) {
                terminal = true;
                plan = Some(DeliveryPlan { source_id: source_id.into(), target_id: target.peer.id.clone(), bytes, reservation: Some(reservation), udp: packet.protocol() == 17 });
                break;
            }
        }
    }
    if !terminal {
        if let Some(target) = peers.values().find(|p| p.peer.ipv4.parse().ok() == Some(packet.dst())) {
            if ipv4::group_route_allowed(Some(source_group), target.group.as_ref()) {
                if packet.protocol() == 17 {
                    if let Some((bytes, reservation)) = nat.prepare_direct(packet, source, &target.peer, now) {
                        plan = Some(DeliveryPlan { source_id: source_id.into(), target_id: target.peer.id.clone(), bytes, reservation: Some(reservation), udp: true });
                    }
                } else {
                    plan = Some(DeliveryPlan { source_id: source_id.into(), target_id: target.peer.id.clone(), bytes: packet.bytes().to_vec(), reservation: None, udp: false });
                }
            }
        }
    }
    (plan, false)
}

async fn drain_pending(socket: &UdpSocket, queue: &mut VecDeque<PendingDelivery>, queued_bytes: &mut usize, peers: &mut HashMap<String, RuntimePeer>, forwards: &[Forward], hub_ip: Option<Ipv4Addr>, nat: &mut Flows, out: &mut [u8]) {
    let mut retry = VecDeque::new();
    while let Some(item) = queue.pop_front() {
        *queued_bytes = queued_bytes.saturating_sub(item.packet.bytes().len());
        // Each item gets a fresh clock reading: prior delivery awaits must not
        // let a later queued item outlive its deadline or reverse-flow expiry.
        let now = Instant::now();
        if item.expired_at(now) { continue; }
        let Some(source) = peers.get(&item.source_id) else { continue };
        let Some(group) = source.group.as_ref() else { continue };
        let source_peer = source.peer.clone();
        let source_group = group.clone();
        let result = if item.reply_only {
            nat.lookup_reply(&item.packet, &source_peer, now).map(|(target_id, bytes, reservation)| DeliveryPlan { source_id: item.source_id.clone(), target_id, bytes, reservation: Some(reservation), udp: item.packet.protocol() == 17 })
        } else {
            resolve_packet(&item.packet, &source_peer, &source_group, &item.source_id, peers, forwards, hub_ip, nat, now).0
        };
        let Some(mut plan) = result else { continue };
        let outcome = deliver_plan(socket, peers, &plan, out).await;
        complete_delivery(nat, peers, &mut plan, outcome);
        if outcome == EgressOutcome::NotReady { retry.push_back(item); }
    }
    for item in retry { enqueue_pending(queue, queued_bytes, item); }
}

fn complete_delivery(nat: &mut Flows, peers: &mut HashMap<String, RuntimePeer>, plan: &mut DeliveryPlan, outcome: EgressOutcome) {
    let delivered = outcome == EgressOutcome::Delivered;
    if let Some(reservation) = plan.reservation.take() { nat.complete(reservation, delivered, Instant::now()); }
    if delivered {
        let n = plan.bytes.len() as u64;
        if let Some(source) = peers.get_mut(&plan.source_id) { source.peer.received_bytes = source.peer.received_bytes.saturating_add(n); }
        if let Some(target) = peers.get_mut(&plan.target_id) { target.peer.sent_bytes = target.peer.sent_bytes.saturating_add(n); }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EgressOutcome { Delivered, Failed, NotReady }

async fn publish_stats(peers: &HashMap<String, RuntimePeer>, stats: &RuntimeStats) {
    let mut snapshot = stats.write().await;
    snapshot.clear();
    snapshot.extend(peers.iter().map(|(id, p)| (id.clone(), (p.peer.received_bytes, p.peer.sent_bytes, p.peer.last_handshake_unix, p.last_data_unix))));
}

async fn deliver_plan(socket:&UdpSocket, peers:&mut HashMap<String,RuntimePeer>, plan:&DeliveryPlan, out:&mut [u8]) -> EgressOutcome {
    let Some(target) = peers.get_mut(&plan.target_id) else { return EgressOutcome::Failed };
    if plan.udp && target.tunnel.time_since_last_handshake().is_none() {
        // Cold UDP delivery is explicitly not accepted: do not pass plaintext
        // into BoringTun's internal queue or commit a flow reservation.
        if let Some(endpoint) = target.endpoint {
            if let TunnResult::WriteToNetwork(initiation) = target.tunnel.format_handshake_initiation(out, false) {
                let _ = socket.send_to(initiation, endpoint).await;
            }
        }
        return EgressOutcome::NotReady;
    }
    let wire = match target.tunnel.encapsulate(&plan.bytes, out) {
        TunnResult::WriteToNetwork(wire) => wire,
        _ => return EgressOutcome::Failed,
    };
    if plan.udp && !matches!(Tunn::parse_incoming_packet(wire), Ok(Packet::PacketData(_))) {
        return EgressOutcome::NotReady;
    }
    let Some(endpoint) = target.endpoint else { return EgressOutcome::Failed };
    if socket.send_to(wire, endpoint).await.is_ok() { EgressOutcome::Delivered } else { EgressOutcome::Failed }
}

fn unix_now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64 }

async fn load_peers(groups:Vec<Group>, current:Vec<Peer>, key:[u8;32], peers:&mut HashMap<String,RuntimePeer>, indexes:&mut HashMap<u32,String>, next:&mut u32, stats:&RuntimeStats)->Result<(),()> {
    let result=install_peers(&groups,current,key,peers,indexes,next,HashMap::new());
    if result.is_ok(){publish_stats(peers,stats).await;} result
}

fn install_peers(groups:&[Group],current:Vec<Peer>,key:[u8;32],peers:&mut HashMap<String,RuntimePeer>,indexes:&mut HashMap<u32,String>,next:&mut u32,old:HashMap<String,RuntimePeer>)->Result<(),()> {
    let group_map:HashMap<_,_>=groups.iter().cloned().map(|g|(g.id.clone(),g)).collect();
    let mut replacements=HashMap::new();let mut new_indexes=HashMap::new();
    for peer in current {
        let group=group_map.get(&peer.group_id).cloned().ok_or(())?;let public=decode_public_key(&peer.public_key)?;
        let prior=old.get(&peer.id).filter(|p|p.peer.public_key==peer.public_key);
        let index=allocate_index(next,&new_indexes)?;let tunnel=Tunn::new(StaticSecret::from(key),PublicKey::from(public),None,None,index>>8,None);
        let mut peer=peer;
        if let Some(prior)=prior { peer.received_bytes=prior.peer.received_bytes;peer.sent_bytes=prior.peer.sent_bytes;peer.last_handshake_unix=prior.peer.last_handshake_unix; }
        new_indexes.insert(index,peer.id.clone());
        replacements.insert(peer.id.clone(),RuntimePeer{peer,group:Some(group),tunnel,endpoint:prior.and_then(|p|p.endpoint),last_data_unix:prior.and_then(|p|p.last_data_unix)});
    }
    *peers=replacements;*indexes=new_indexes;Ok(())
}

fn validate_persisted_addresses(snapshot:&crate::storage::RuntimeSnapshot)->Result<(),()> {
    let peers=&snapshot.peers;
    let forwards=&snapshot.forwards;
    if peers.is_empty()&&forwards.is_empty() { return Ok(()); }
    let settings=snapshot.settings.as_ref().ok_or(())?;let subnet=Subnet24::parse(&settings.subnet).map_err(|_|())?;
    for peer in peers {let ip:Ipv4Addr=peer.ipv4.parse().map_err(|_|())?;if subnet.peer_ip(ip.octets()[3]).as_deref()!=Some(peer.ipv4.as_str()){return Err(())}}
    for forward in forwards {if !matches!(forward.protocol.as_str(),"tcp"|"udp")||forward.target_port==0{return Err(())}}
    Ok(())
}

fn allocate_index(next: &mut u32, indexes: &HashMap<u32, String>) -> Result<u32, ()> {
    for _ in 0..(u16::MAX as usize) {
        let candidate = (*next).wrapping_add(1).max(1) & 0x00ff_ffff;
        *next = candidate;
        let wire_index = candidate << 8;
        if !indexes.contains_key(&wire_index) { return Ok(wire_index); }
    }
    Err(())
}

fn decode_public_key(encoded: &str) -> Result<[u8; 32], ()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ())?;
    bytes.try_into().map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Group;
    use boringtun::x25519::StaticSecret;
    use tokio::time::timeout;
    use base64::Engine;
    fn create_forward_for_test(store:&Store,mut forward:Forward){store.create_forward(&mut forward).unwrap();}

    fn pending_packet(payload_len: usize) -> ipv4::ValidatedPacket {
        let raw=service_packet(17,[10,77,0,2],[10,77,0,3],1234,5678,0,&vec![0;payload_len]);
        let source=Peer{id:"a".into(),name:"a".into(),public_key:String::new(),ipv4:"10.77.0.2".into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let group=Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]};
        ipv4::validate(&raw,&source,Some(&group)).unwrap()
    }

    #[test]
    fn app_pending_queue_enforces_entry_byte_and_deadline_bounds() {
        let now=Instant::now();
        let packet=pending_packet(0);
        let mut queue=VecDeque::new(); let mut bytes=0;
        for _ in 0..=PENDING_LIMIT {
            enqueue_pending(&mut queue,&mut bytes,PendingDelivery{source_id:"a".into(),packet:packet.clone(),reply_only:false,deadline:now+PENDING_TTL});
        }
        assert_eq!(queue.len(),PENDING_LIMIT);
        assert_eq!(bytes,PENDING_LIMIT*packet.bytes().len());

        let large=pending_packet(60_000);
        let mut queue=VecDeque::new(); let mut bytes=0;
        for _ in 0..20 {
            enqueue_pending(&mut queue,&mut bytes,PendingDelivery{source_id:"a".into(),packet:large.clone(),reply_only:false,deadline:now+PENDING_TTL});
        }
        assert_eq!(queue.len(),PENDING_BYTES/large.bytes().len());
        assert!(bytes<=PENDING_BYTES);

        let delivery=PendingDelivery{source_id:"a".into(),packet,reply_only:false,deadline:now+PENDING_TTL};
        assert!(!delivery.expired_at(now+PENDING_TTL-Duration::from_millis(1)));
        assert!(delivery.expired_at(now+PENDING_TTL));
    }

    #[test]
    fn persisted_peer_address_validation_accepts_last_peer_ip_and_rejects_reserved_addresses() {
        for (ip, expected) in [("10.77.0.254", true), ("10.77.0.1", false), ("10.77.0.255", false)] {
            let store=Store::open(":memory:").unwrap();
            store.setup("10.77.0.0/24","hub.example:51820",25).unwrap();
            store.add_group(&Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]}).unwrap();
            store.add_peer(&Peer{id:"p".into(),name:"p".into(),public_key:"key".into(),ipv4:ip.into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
            let mut snapshot=store.runtime_snapshot().unwrap();
            snapshot.forwards.clear();
            assert_eq!(validate_persisted_addresses(&snapshot).is_ok(),expected,"{ip}");
        }
    }

    #[test]
    fn forward_requires_both_allowlist_and_directed_backend_acl() {
        let forward=Forward{id:"f".into(),name:"f".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:8080,allowed_group_ids:vec!["a".into()]};
        let group=Group{id:"a".into(),name:"a".into(),allowed_groups:vec!["b".into()]};
        assert!(policy::forward_allowed(&forward,"a",Some(&group),"b"));
        assert!(!policy::forward_allowed(&forward,"c",Some(&group),"b"));
        let denied=Group{allowed_groups:vec![],..group};
        assert!(!policy::forward_allowed(&forward,"a",Some(&denied),"b"));
    }
    #[test]
    fn receiver_indexes_are_unique() {
        let mut next = 0;
        let mut map = HashMap::new();
        let first = allocate_index(&mut next, &map).unwrap();
        map.insert(first, "a".into());
        let second = allocate_index(&mut next, &map).unwrap();
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn forward_load_errors_fail_closed_and_are_acknowledged() {
        async fn broken_store(path: &std::path::Path) -> Arc<Store> {
            let store=Arc::new(Store::open(path.to_str().unwrap()).unwrap());
            store.setup("10.77.0.0/24","hub.example:51820",25).unwrap();
            store.add_group(&Group{id:"g".into(),name:"g".into(),allowed_groups:vec![]}).unwrap();
            let secret=StaticSecret::from([41u8;32]);
            store.add_peer(&Peer{id:"p".into(),name:"p".into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret).as_bytes()),ipv4:"10.77.0.2".into(),group_id:"g".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
            create_forward_for_test(&store,Forward{id:"f".into(),name:"f".into(),protocol:"udp".into(),target_peer_id:"p".into(),target_port:53,allowed_group_ids:vec!["g".into()]});
            store
        }
        let dir=tempfile::tempdir().unwrap();
        let startup_store=broken_store(&dir.path().join("startup.sqlite")).await;
        rusqlite::Connection::open(dir.path().join("startup.sqlite")).unwrap().execute("UPDATE forwards SET allowed='not-json'",[]).unwrap();
        let socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (_tx,rx)=mpsc::channel(1);let (ready,wait)=oneshot::channel();
        tokio::spawn(run_udp(socket,startup_store,[42;32],rx,RuntimeStats::default(),Some(ready)));
        assert!(wait.await.unwrap().is_err(),"startup reports invalid persisted forward state");

        let reload_store=broken_store(&dir.path().join("reload.sqlite")).await;
        let socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (tx,rx)=mpsc::channel(1);let (ready,wait)=oneshot::channel();
        tokio::spawn(run_udp(socket,reload_store.clone(),[43;32],rx,RuntimeStats::default(),Some(ready)));
        wait.await.unwrap().unwrap();
        rusqlite::Connection::open(dir.path().join("reload.sqlite")).unwrap().execute("UPDATE forwards SET allowed='not-json'",[]).unwrap();
        let (ack,ack_wait)=oneshot::channel();tx.send(ReloadCommand{ack}).await.unwrap();
        assert!(ack_wait.await.unwrap().is_err(),"reload reports invalid persisted forward state");
    }

    async fn establish_client(socket: &UdpSocket, address: SocketAddr, client: &mut Tunn, tx: &mut [u8], rx: &mut [u8]) {
        let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],tx) else { panic!("expected handshake initiation") };
        socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),socket.recv_from(rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2,"hub responds to authenticated initiation");
        if let TunnResult::WriteToNetwork(reply)=client.decapsulate(None,&rx[..n],tx) { socket.send_to(reply,address).await.unwrap(); }
    }

    fn transport_checksum(src: [u8; 4], dst: [u8; 4], proto: u8, segment: &[u8]) -> u16 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&src); bytes.extend_from_slice(&dst);
        bytes.extend_from_slice(&[0, proto]); bytes.extend_from_slice(&(segment.len() as u16).to_be_bytes());
        bytes.extend_from_slice(segment);
        let mut sum = 0u32;
        for c in bytes.chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        if bytes.len() % 2 != 0 { sum += (bytes[bytes.len()-1] as u32) << 8; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        !(sum as u16)
    }

    fn service_packet(proto: u8, src: [u8; 4], dst: [u8; 4], sport: u16, dport: u16, flags: u8, payload: &[u8]) -> Vec<u8> {
        let transport_len = (if proto == 6 { 20 } else { 8 }) + payload.len();
        let mut p = vec![0; 20 + transport_len];
        let packet_len = p.len() as u16;
        p[0] = 0x45; p[2..4].copy_from_slice(&packet_len.to_be_bytes()); p[8] = 64; p[9] = proto;
        p[12..16].copy_from_slice(&src); p[16..20].copy_from_slice(&dst);
        p[20..22].copy_from_slice(&sport.to_be_bytes()); p[22..24].copy_from_slice(&dport.to_be_bytes());
        if proto == 6 { p[32] = 0x50; p[33] = flags; p[40..].copy_from_slice(payload); }
        else { p[24..26].copy_from_slice(&(transport_len as u16).to_be_bytes()); p[28..].copy_from_slice(payload); }
        let c = transport_checksum(src, dst, proto, &p[20..]);
        if proto == 6 { p[36..38].copy_from_slice(&c.to_be_bytes()); } else { p[26..28].copy_from_slice(&c.to_be_bytes()); }
        let mut sum = 0u32; for c in p[..20].chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        p[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes()); p
    }

    fn assert_packet_checksums(p: &[u8]) {
        let mut sum = 0u32; for c in p[..20].chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        assert_eq!(!(sum as u16), 0, "IPv4 checksum");
        let proto = p[9]; let checksum = if proto == 6 { u16::from_be_bytes([p[36], p[37]]) } else { u16::from_be_bytes([p[26], p[27]]) };
        if proto == 6 || checksum != 0 { assert_eq!(transport_checksum(p[12..16].try_into().unwrap(), p[16..20].try_into().unwrap(), proto, &p[20..]), 0, "TCP/UDP checksum"); }
    }

    async fn send_inner(socket: &UdpSocket, address: SocketAddr, client: &mut Tunn, packet: &[u8], tx: &mut [u8]) {
        let TunnResult::WriteToNetwork(wire) = client.encapsulate(packet, tx) else { panic!("expected encrypted packet") };
        socket.send_to(wire, address).await.unwrap();
    }

    async fn recv_inner(socket: &UdpSocket, client: &mut Tunn, rx: &mut [u8], tx: &mut [u8]) -> Vec<u8> {
        loop {
            let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(rx)).await.unwrap().unwrap();
            match client.decapsulate(None, &rx[..n], tx) {
                TunnResult::WriteToTunnelV4(p, _) => return p.to_vec(),
                TunnResult::WriteToNetwork(p) => { /* handshake response; caller needs a socket to send it */ let _ = p; panic!("unexpected hub handshake while awaiting packet"); }
                _ => {}
            }
        }
    }

    async fn assert_no_inner(socket: &UdpSocket, address: SocketAddr, client: &mut Tunn, rx: &mut [u8], tx: &mut [u8]) {
        let until = tokio::time::Instant::now() + Duration::from_millis(250);
        loop {
            let remaining = until.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { return; }
            let Ok(Ok((n,_))) = timeout(remaining, socket.recv_from(rx)).await else { return };
            match client.decapsulate(None,&rx[..n],tx) {
                TunnResult::WriteToTunnelV4(_,_) => panic!("denied flow delivered application IPv4"),
                TunnResult::WriteToNetwork(packet) => { let _=socket.send_to(packet,address).await; },
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn boringtun_nat_tcp_udp_roundtrip_and_live_policy_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("nat-e2e.sqlite").to_str().unwrap()).unwrap());
        store.setup("192.168.44.0/24","hub.example:51820",25).unwrap();
        for g in [
            Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]},
            Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]},
            Group{id:"denied".into(),name:"denied".into(),allowed_groups:vec![]},
        ] { store.add_group(&g).unwrap(); }
        store.set_acl("clients", &["backend".into()]).unwrap();
        // Isolate the forward allowlist assertion: C's group ACL itself permits
        // access to the backend, but its group is not in the forward allowlist.
        store.set_acl("denied", &["backend".into()]).unwrap();
        let secrets = [StaticSecret::from([31u8;32]), StaticSecret::from([32u8;32]), StaticSecret::from([33u8;32]), StaticSecret::from([34u8;32])];
        let specs = [("a","192.168.44.2","clients"),("b","192.168.44.3","backend"),("c","192.168.44.4","denied"),("d","192.168.44.5","backend")];
        for ((id,ip,group), secret) in specs.iter().zip(secrets.iter()) {
            store.add_peer(&Peer{id:(*id).into(),name:(*id).into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:(*ip).into(),group_id:(*group).into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        create_forward_for_test(&store,Forward{id:"f".into(),name:"service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:8080,allowed_group_ids:vec!["clients".into()]});
        create_forward_for_test(&store,Forward{id:"fu".into(),name:"udp service".into(),protocol:"udp".into(),target_peer_id:"b".into(),target_port:8080,allowed_group_ids:vec!["clients".into()]});
        create_forward_for_test(&store,Forward{id:"reserved".into(),name:"reserved tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:40000,allowed_group_ids:vec!["clients".into()]});
        let hub_private=[35u8;32]; let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(4); let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Some(ready))); ready_rx.await.unwrap().unwrap();
        let sockets = [UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap(),UdpSocket::bind("127.0.0.1:0").await.unwrap()];
        let mut clients: Vec<Tunn> = secrets.into_iter().enumerate().map(|(i,s)|Tunn::new(s,hub_public,None,None,60+i as u32,None)).collect();
        let mut tx=vec![0;65535]; let mut rx=vec![0;65535];
        for i in 0..4 { establish_client(&sockets[i],address,&mut clients[i],&mut tx,&mut rx).await; }

        let mut translated_reply = None;
        let mut total_request_bytes=0u64;
        let mut total_reply_bytes=0u64;
        let mut requests=Vec::new();
        for (_proto,number,flags) in [("tcp",6,0x02),("udp",17,0)] {
            let req=service_packet(number,[192,168,44,2],[192,168,44,1],12345,8080,flags,b"request");
            total_request_bytes+=req.len() as u64;
            let src_peer=store.peers().unwrap().into_iter().find(|p|p.id=="a").unwrap(); let source_group=store.group("clients").unwrap().unwrap();
            assert!(ipv4::validate_and_forward(&req,&src_peer,Some(&source_group)).is_some(),"fixture must pass authenticated IPv4 validation");
            send_inner(&sockets[0],address,&mut clients[0],&req,&mut tx).await;
            requests.push((number,req));
        }
        // Both requests are outstanding before either backend reply arrives.
        let mut mapped_requests=Vec::new();
        for (number,req) in &requests {
            let translated=recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx).await;
            assert_eq!(&translated[12..16],&[192,168,44,1]); assert_eq!(&translated[16..20],&[192,168,44,3]);
            let snat_port=u16::from_be_bytes([translated[20],translated[21]]);
            assert_eq!(snat_port,if *number==6{40001}else{40000},"initial protocol-specific SNAT allocation"); assert_eq!(u16::from_be_bytes([translated[22],translated[23]]),8080);
            assert_eq!(translated[8],63,"SNAT/DNAT must not decrement TTL a second time"); assert_packet_checksums(&translated);
            assert_eq!(translated[9],*number); let payload_at=if *number==6{40}else{28}; assert_eq!(&translated[payload_at..],&req[payload_at..]);
            mapped_requests.push((*number,snat_port));
        }
        for (number,snat_port) in mapped_requests.iter().rev() {
            let reply=service_packet(*number,[192,168,44,3],[192,168,44,1],8080,*snat_port,if *number==6{0x12}else{0},b"reply");
            total_reply_bytes+=reply.len() as u64;
            translated_reply=Some(reply.clone());
            send_inner(&sockets[1],address,&mut clients[1],&reply,&mut tx).await;
        }
        for (number,_) in requests.iter().rev() {
            let restored=recv_inner(&sockets[0],&mut clients[0],&mut rx,&mut tx).await;
            assert_eq!(&restored[12..16],&[192,168,44,1]); assert_eq!(&restored[16..20],&[192,168,44,2]);
            assert_eq!(u16::from_be_bytes([restored[20],restored[21]]),8080); assert_eq!(u16::from_be_bytes([restored[22],restored[23]]),12345);
            assert_eq!(restored[9],*number); let payload_at=if *number==6{40}else{28}; assert_eq!(&restored[payload_at..],b"reply");
            assert_eq!(restored[8],63); assert_packet_checksums(&restored);
        }
        // Warm direct UDP is stateful in both directions and admits only the
        // exact authenticated reverse tuple.
        let direct = service_packet(17,[192,168,44,2],[192,168,44,3],12345,9090,0,b"direct");
        send_inner(&sockets[0],address,&mut clients[0],&direct,&mut tx).await;
        let delivered = recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx).await;
        assert_eq!(&delivered[12..20],&direct[12..20]);
        total_request_bytes += direct.len() as u64;
        let direct_reply = service_packet(17,[192,168,44,3],[192,168,44,2],9090,12345,0,b"direct-reply");
        send_inner(&sockets[1],address,&mut clients[1],&direct_reply,&mut tx).await;
        let restored = recv_inner(&sockets[0],&mut clients[0],&mut rx,&mut tx).await;
        assert_eq!(&restored[12..20],&direct_reply[12..20]);
        total_reply_bytes += direct_reply.len() as u64;
        for invalid in [
            service_packet(17,[192,168,44,3],[192,168,44,2],9091,12345,0,b"changed-source-port"),
            service_packet(17,[192,168,44,3],[192,168,44,2],9090,12346,0,b"changed-destination-port"),
        ] {
            send_inner(&sockets[1],address,&mut clients[1],&invalid,&mut tx).await;
            assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        }
        // Counters are plaintext IPv4 bytes delivered across the hub boundary:
        // ingress accounts on the source peer, egress on the destination peer.
        let snapshot=stats.read().await;
        let (a_rx,a_tx,_,_)=snapshot["a"];
        let (b_rx,b_tx,_,_)=snapshot["b"];
        assert_eq!(a_rx,total_request_bytes,"A ingress includes both delivered forward requests");
        assert_eq!(a_tx,total_reply_bytes,"A egress includes both delivered reverse replies");
        assert_eq!(b_rx,total_reply_bytes,"B ingress includes both delivered reverse replies");
        assert_eq!(b_tx,total_request_bytes,"B egress includes both delivered forward requests");
        drop(snapshot);

        // A newly added TCP service is installed only after acknowledged reload;
        // mappings are flushed and the next TCP candidate is also excluded.
        create_forward_for_test(&store,Forward{id:"reserved-next".into(),name:"next reserved tcp service".into(),protocol:"tcp".into(),target_peer_id:"b".into(),target_port:40001,allowed_group_ids:vec!["clients".into()]});
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        clients[0]=Tunn::new(StaticSecret::from([31u8;32]),hub_public,None,None,60,None);
        clients[1]=Tunn::new(StaticSecret::from([32u8;32]),hub_public,None,None,61,None);
        establish_client(&sockets[0],address,&mut clients[0],&mut tx,&mut rx).await;
        establish_client(&sockets[1],address,&mut clients[1],&mut tx,&mut rx).await;
        clients[2]=Tunn::new(StaticSecret::from([33u8;32]),hub_public,None,None,62,None);
        clients[3]=Tunn::new(StaticSecret::from([34u8;32]),hub_public,None,None,63,None);
        establish_client(&sockets[2],address,&mut clients[2],&mut tx,&mut rx).await;
        establish_client(&sockets[3],address,&mut clients[3],&mut tx,&mut rx).await;
        send_inner(&sockets[1],address,&mut clients[1],translated_reply.as_ref().unwrap(),&mut tx).await;
        assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        for (number,flags) in [(6,0x02),(17,0)] {
            let request=service_packet(number,[192,168,44,2],[192,168,44,1],12346,8080,flags,b"after-reload");
            send_inner(&sockets[0],address,&mut clients[0],&request,&mut tx).await;
            let translated=recv_inner(&sockets[1],&mut clients[1],&mut rx,&mut tx).await;
            let snat = u16::from_be_bytes([translated[20],translated[21]]);
            if number == 6 { assert!(![40000, 40001].contains(&snat), "reserved TCP service ports are excluded"); }
            else { assert_eq!(snat, 40000, "UDP may use a port reserved only by TCP"); }
        }

        // C has a directed backend ACL, but the current forward allowlist excludes it.
        let denied=service_packet(17,[192,168,44,4],[192,168,44,1],2345,8080,0,b"denied");
        send_inner(&sockets[2],address,&mut clients[2],&denied,&mut tx).await;
        assert!(timeout(Duration::from_millis(200),sockets[1].recv_from(&mut rx)).await.is_err());
        // D is freshly authenticated after policy reload, but spoofing B's inner
        // source is rejected before reverse NAT.
        let spoof=service_packet(17,[192,168,44,3],[192,168,44,1],8080,40000,0,b"spoof");
        send_inner(&sockets[3],address,&mut clients[3],&spoof,&mut tx).await;
        assert!(timeout(Duration::from_millis(200),sockets[0].recv_from(&mut rx)).await.is_err());

        // ACL revoke is synchronously acknowledged and flushes NAT state.
        store.set_acl("clients", &[]).unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        clients[0]=Tunn::new(StaticSecret::from([31u8;32]),hub_public,None,None,60,None);
        clients[1]=Tunn::new(StaticSecret::from([32u8;32]),hub_public,None,None,61,None);
        establish_client(&sockets[0],address,&mut clients[0],&mut tx,&mut rx).await;
        establish_client(&sockets[1],address,&mut clients[1],&mut tx,&mut rx).await;
        // The reload must clear established NAT mappings: a reply translated for
        // the previous flow cannot be delivered to A using its stale mapping.
        send_inner(&sockets[1],address,&mut clients[1],translated_reply.as_ref().unwrap(),&mut tx).await;
        assert_no_inner(&sockets[0],address,&mut clients[0],&mut rx,&mut tx).await;
        send_inner(&sockets[0],address,&mut clients[0],&service_packet(17,[192,168,44,2],[192,168,44,1],12345,8080,0,b"revoked"),&mut tx).await;
        assert_no_inner(&sockets[1],address,&mut clients[1],&mut rx,&mut tx).await;
        // Restore ACL, then remove the forward and prove its acknowledged removal blocks traffic.
        store.set_acl("clients", &["backend".into()]).unwrap(); store.remove_forward("fu").unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        clients[0]=Tunn::new(StaticSecret::from([31u8;32]),hub_public,None,None,60,None);
        establish_client(&sockets[0],address,&mut clients[0],&mut tx,&mut rx).await;
        send_inner(&sockets[0],address,&mut clients[0],&service_packet(17,[192,168,44,2],[192,168,44,1],12345,8080,0,b"removed"),&mut tx).await;
        assert_no_inner(&sockets[1],address,&mut clients[1],&mut rx,&mut tx).await;
    }

    #[tokio::test]
    async fn only_authenticated_keepalive_can_migrate_peer_endpoint() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("endpoint.sqlite").to_str().unwrap()).unwrap());
        store.setup("10.1.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"a".into(),name:"A".into(),allowed_groups:vec![]}).unwrap();
        store.add_group(&Group{id:"b".into(),name:"B".into(),allowed_groups:vec!["a".into()]}).unwrap();
        store.set_acl("b", &["a".into()]).unwrap();
        let a_secret=StaticSecret::from([17u8;32]);
        let b_secret=StaticSecret::from([18u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.1.0.2","a"),("b",&b_secret,"10.1.0.3","b")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        let hub_private=[19u8;32];
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(1);
        let (ready,ready_rx)=oneshot::channel();
        tokio::spawn(run_udp(server,store,hub_private,commands_rx,RuntimeStats::default(),Some(ready)));
        ready_rx.await.unwrap().unwrap();
        let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let original=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let migrated=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let attacker=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret,hub_public,None,None,51,None);
        let mut b=Tunn::new(b_secret,hub_public,None,None,52,None);
        let mut tx=vec![0u8;65535]; let mut rx=vec![0u8;65535];
        establish_client(&original,address,&mut a,&mut tx,&mut rx).await;
        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;

        // A valid encrypted empty transport packet is an authenticated keepalive.
        let TunnResult::WriteToNetwork(keepalive)=a.encapsulate(&[],&mut tx) else {panic!("expected encrypted keepalive")};
        let keepalive=keepalive.to_vec();
        migrated.send_to(&keepalive,address).await.unwrap();
        time::sleep(Duration::from_millis(50)).await;

        let route_to_a=ipv4::test_packet([10,1,0,3],[10,1,0,2],false,false);
        let TunnResult::WriteToNetwork(wire)=b.encapsulate(&route_to_a,&mut tx) else {panic!("expected encrypted routed packet")};
        b_socket.send_to(wire,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),migrated.recv_from(&mut rx)).await.unwrap().unwrap();
        assert!(matches!(a.decapsulate(None,&rx[..n],&mut tx),TunnResult::WriteToTunnelV4(_, _)),"hub uses the new endpoint after authenticated keepalive");
        assert!(timeout(Duration::from_millis(100),original.recv_from(&mut rx)).await.is_err(),"hub stopped using the old endpoint");

        // An invalid tag and an exact replay must not change the learned endpoint.
        let TunnResult::WriteToNetwork(forged)=a.encapsulate(&[],&mut tx) else {panic!("expected keepalive for forgery")};
        let mut forged=forged.to_vec(); *forged.last_mut().unwrap()^=1;
        attacker.send_to(&forged,address).await.unwrap();
        time::sleep(Duration::from_millis(50)).await;
        attacker.send_to(&keepalive,address).await.unwrap();
        time::sleep(Duration::from_millis(50)).await;
        let TunnResult::WriteToNetwork(wire)=b.encapsulate(&route_to_a,&mut tx) else {panic!("expected encrypted routed packet")};
        b_socket.send_to(wire,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),migrated.recv_from(&mut rx)).await.unwrap().unwrap();
        assert!(matches!(a.decapsulate(None,&rx[..n],&mut tx),TunnResult::WriteToTunnelV4(_, _)),"invalid and replayed packets cannot hijack the endpoint");
        assert!(timeout(Duration::from_millis(100),attacker.recv_from(&mut rx)).await.is_err(),"forged/replayed sender receives no routed packet");
        drop(commands_tx);
    }

    #[tokio::test]
    async fn boringtun_clients_handshake_and_route_only_authorized_ipv4() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("test.sqlite").to_str().unwrap()).unwrap());
        store.setup("10.77.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group { id:"a".into(), name:"A".into(), allowed_groups:vec!["b".into()] }).unwrap();
        store.add_group(&Group { id:"b".into(), name:"B".into(), allowed_groups:vec![] }).unwrap();
        store.set_acl("a", &["b".into()]).unwrap();
        let client_a_secret = StaticSecret::from([7u8;32]);
        let client_b_secret = StaticSecret::from([8u8;32]);
        let client_c_secret = StaticSecret::from([10u8;32]);
        for (id, secret, ip, group) in [("a", &client_a_secret, "10.77.0.2", "a"), ("b", &client_b_secret, "10.77.0.3", "b"), ("c", &client_c_secret, "10.77.0.4", "a")] {
            let public = PublicKey::from(secret);
            let peer = Peer { id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(public.as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None };
            store.add_peer(&peer).unwrap();
        }
        let hub_private=[9u8;32];
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address=server.local_addr().unwrap();
        let (commands_tx, commands_rx)=mpsc::channel(4);
        let stats=RuntimeStats::default();
        let (ready,ready_rx)=oneshot::channel();tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Some(ready)));
        ready_rx.await.unwrap().unwrap();
        let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let client_a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_b_migrated_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_c_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut client_a=Tunn::new(client_a_secret.clone(),hub_public,None,None,33,None);
        let mut client_b=Tunn::new(client_b_secret.clone(),hub_public,None,None,34,None);
        let mut client_c=Tunn::new(client_c_secret,hub_public,None,None,35,None);
        let mut tx=vec![0u8;65535]; let mut rx=vec![0u8;65535];
        for (socket, client) in [(&client_a_socket, &mut client_a), (&client_b_socket, &mut client_b)] {
            let TunnResult::WriteToNetwork(init)=client.encapsulate(&[],&mut tx) else { panic!("expected handshake initiation") };
            socket.send_to(init,address).await.unwrap();
            let (n,_) = timeout(Duration::from_secs(3),socket.recv_from(&mut rx)).await.unwrap().unwrap();
            assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2,"server must return a handshake response");
            let result=client.decapsulate(None,&rx[..n],&mut tx);
            match result {
                TunnResult::Done => {},
                TunnResult::WriteToNetwork(packet) => { socket.send_to(packet,address).await.unwrap(); },
                other => panic!("client rejected handshake response: {other:?}"),
            }
        }
        let TunnResult::WriteToNetwork(init)=client_c.encapsulate(&[],&mut tx) else {panic!("expected client C handshake initiation")};
        client_c_socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),client_c_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2,"server must return client C handshake response");
        if let TunnResult::WriteToNetwork(packet)=client_c.decapsulate(None,&rx[..n],&mut tx) { client_c_socket.send_to(packet,address).await.unwrap(); }

        // An unrelated database mutation resets tunnel sessions, but must not
        // discard surviving authenticated destinations; initiators can re-key.
        store.add_group(&Group{id:"unrelated".into(),name:"unrelated".into(),allowed_groups:vec![]}).unwrap();
        let (ack,wait)=oneshot::channel();commands_tx.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();
        client_a=Tunn::new(client_a_secret.clone(),hub_public,None,None,33,None);
        client_b=Tunn::new(client_b_secret.clone(),hub_public,None,None,34,None);
        let TunnResult::WriteToNetwork(init)=client_a.encapsulate(&[],&mut tx) else {panic!("expected post-reload source handshake")};
        client_a_socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),client_a_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2);
        if let TunnResult::WriteToNetwork(reply)=client_a.decapsulate(None,&rx[..n],&mut tx) { client_a_socket.send_to(reply,address).await.unwrap(); }

        let packet=ipv4::test_packet([10,77,0,2],[10,77,0,3],false,false);
        let TunnResult::WriteToNetwork(wire)=client_a.encapsulate(&packet,&mut tx) else {panic!("expected encrypted data")};
        client_a_socket.send_to(wire,address).await.unwrap();
        // B has sent no application traffic since reload. The hub initiates
        // B's new tunnel and retains the owned UDP packet outside BoringTun.
        let (n,_) = timeout(Duration::from_secs(3),client_b_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),1,"hub initiates the idle destination handshake");
        let TunnResult::WriteToNetwork(reply)=client_b.decapsulate(None,&rx[..n],&mut tx) else {panic!("destination rejected hub handshake")};
        // The authenticated handshake response comes from a new UDP socket,
        // migrating B away from the endpoint saved before the reload.
        client_b_migrated_socket.send_to(reply,address).await.unwrap();
        let during_handshake=stats.read().await;
        assert_eq!(during_handshake["a"].0,0,"handshake-only traffic is not counted as application delivery");
        assert_eq!(during_handshake["b"].1,0,"handshake-only traffic is not counted as application delivery");
        drop(during_handshake);
        let TunnResult::WriteToNetwork(keepalive)=client_b.encapsulate(&[],&mut tx) else {panic!("expected authenticated transport keepalive")};
        client_b_migrated_socket.send_to(keepalive,address).await.unwrap();
        let first_inner=recv_inner(&client_b_migrated_socket,&mut client_b,&mut rx,&mut tx).await;
        assert_eq!(&first_inner[12..20],&packet[12..20]);
        assert_eq!(first_inner[8],63,"pending forwarding decrements TTL only once");
        let after_data=stats.read().await;
        assert_eq!(after_data["a"].0,packet.len() as u64);
        assert_eq!(after_data["b"].1,packet.len() as u64);
        drop(after_data);

        // The pending delivery used the authenticated response endpoint; warm
        // traffic continues to route there without another handshake.
        let second_packet=ipv4::test_packet([10,77,0,2],[10,77,0,3],false,false);
        let TunnResult::WriteToNetwork(wire)=client_a.encapsulate(&second_packet,&mut tx) else {panic!("expected second encrypted packet")};
        client_a_socket.send_to(wire,address).await.unwrap();
        let second_inner=loop {
            let (n,_) = timeout(Duration::from_secs(3),client_b_migrated_socket.recv_from(&mut rx)).await.unwrap().unwrap();
            match client_b.decapsulate(None,&rx[..n],&mut tx) {
                TunnResult::WriteToTunnelV4(packet,_) => break packet.to_vec(),
                TunnResult::WriteToNetwork(reply) => { client_b_migrated_socket.send_to(reply,address).await.unwrap(); }
                _ => {}
            }
        };
        assert_eq!(&second_inner[12..20],&second_packet[12..20]);
        assert_eq!(second_inner[8],63,"forwarding decrements TTL");
        assert!(timeout(Duration::from_millis(100),client_b_socket.recv_from(&mut rx)).await.is_err(),"hub continued using the old destination endpoint");

        // Reverse direction is denied, as are same-group routing, source spoofing,
        // malformed headers and both first and non-first IPv4 fragments.
        for (source, destination, same_group, _spoof, malformed, fragment) in [
            ([10,77,0,3],[10,77,0,2],false,false,false,false),
            ([10,77,0,2],[10,77,0,4],true,false,false,false),
            ([10,77,0,99],[10,77,0,3],false,true,false,false),
            ([10,77,0,2],[10,77,0,3],false,false,true,false),
            ([10,77,0,2],[10,77,0,3],false,false,false,true),
        ] {
            let (socket, client) = if source[3] == 3 { (&client_b_socket,&mut client_b) } else { (&client_a_socket,&mut client_a) };
            let packet=ipv4::test_packet(source,destination,malformed,fragment);
            let TunnResult::WriteToNetwork(wire)=client.encapsulate(&packet,&mut tx) else {panic!("expected encrypted packet")};
            socket.send_to(wire,address).await.unwrap();
            let denied_socket=match destination[3] {2=>&client_a_socket,3=>&client_b_socket,4=>&client_c_socket,_=>unreachable!()};
            assert!(timeout(Duration::from_millis(150),denied_socket.recv_from(&mut rx)).await.is_err(),"denied packet unexpectedly routed (same_group={same_group})");
        }

        // A successful acknowledgement means the old tunnels and policy are gone.
        store.set_acl("a", &[]).unwrap();
        let (ack,wait)=oneshot::channel();commands_tx.send(ReloadCommand{ack}).await.unwrap();wait.await.unwrap().unwrap();
        // The initiating client establishes a fresh session after policy reload.
        client_a=Tunn::new(client_a_secret,hub_public,None,None,33,None);
        let TunnResult::WriteToNetwork(init)=client_a.encapsulate(&[],&mut tx) else {panic!("expected post-reload handshake")};
        client_a_socket.send_to(init,address).await.unwrap();
        let (n,_) = timeout(Duration::from_secs(3),client_a_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),2);
        if let TunnResult::WriteToNetwork(packet)=client_a.decapsulate(None,&rx[..n],&mut tx) { client_a_socket.send_to(packet,address).await.unwrap(); }
        let TunnResult::WriteToNetwork(wire)=client_a.encapsulate(&packet,&mut tx) else {panic!("expected encrypted packet")};
        client_a_socket.send_to(wire,address).await.unwrap();
        assert!(timeout(Duration::from_millis(150),client_b_socket.recv_from(&mut rx)).await.is_err(),"revoked ACL remained active past the refresh window");

        let unknown=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let unknown_secret=StaticSecret::from([11u8;32]);
        let mut unknown_client=Tunn::new(unknown_secret,hub_public,None,None,35,None);
        let TunnResult::WriteToNetwork(init)=unknown_client.encapsulate(&[],&mut tx) else {panic!("expected initiation")};
        unknown.send_to(init,address).await.unwrap();
        assert!(timeout(Duration::from_millis(150),unknown.recv_from(&mut rx)).await.is_err(),"unknown static key must not receive a response");
    }

    #[tokio::test]
    async fn cold_forward_udp_is_delivered_only_after_authenticated_target_handshake() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("cold-forward.sqlite").to_str().unwrap()).unwrap());
        store.setup("10.88.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"clients".into(),name:"clients".into(),allowed_groups:vec!["backend".into()]}).unwrap();
        store.add_group(&Group{id:"backend".into(),name:"backend".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("clients", &["backend".into()]).unwrap();
        let a_secret=StaticSecret::from([81u8;32]); let b_secret=StaticSecret::from([82u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.88.0.2","clients"),("b",&b_secret,"10.88.0.3","backend")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        create_forward_for_test(&store,Forward{id:"udp".into(),name:"udp service".into(),protocol:"udp".into(),target_peer_id:"b".into(),target_port:5353,allowed_group_ids:vec!["clients".into()]});
        let hub_private=[83u8;32]; let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(2); let (ready,ready_rx)=oneshot::channel();
        let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Some(ready))); ready_rx.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret.clone(),hub_public,None,None,81,None); let mut b=Tunn::new(b_secret.clone(),hub_public,None,None,82,None);
        let mut tx=vec![0;65535]; let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;
        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;

        // Reload resets the tunnel sessions but preserves each authenticated endpoint.
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        a=Tunn::new(a_secret,hub_public,None,None,81,None); b=Tunn::new(b_secret,hub_public,None,None,82,None);
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;
        let request=service_packet(17,[10,88,0,2],[10,88,0,1],12345,5353,0,b"cold-forward");
        send_inner(&a_socket,address,&mut a,&request,&mut tx).await;
        let (n,_)=timeout(Duration::from_secs(3),b_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),1,"cold forward UDP starts a destination handshake");
        let TunnResult::WriteToNetwork(response)=b.decapsulate(None,&rx[..n],&mut tx) else {panic!("target rejected forward handshake")};
        b_socket.send_to(response,address).await.unwrap();
        let TunnResult::WriteToNetwork(keepalive)=b.encapsulate(&[],&mut tx) else {panic!("expected authenticated keepalive")};
        b_socket.send_to(keepalive,address).await.unwrap();
        let delivered=recv_inner(&b_socket,&mut b,&mut rx,&mut tx).await;
        assert_eq!(&delivered[12..16],&[10,88,0,1]); assert_eq!(&delivered[16..20],&[10,88,0,3]);
        assert_eq!(u16::from_be_bytes([delivered[22],delivered[23]]),5353);
        assert_eq!(&delivered[28..],b"cold-forward");
        assert_eq!(delivered[8],63,"forward packet TTL is decremented exactly once");
        let snapshot=stats.read().await;
        assert_eq!(snapshot["a"].0,request.len() as u64);
        assert_eq!(snapshot["b"].1,delivered.len() as u64);
    }

    #[tokio::test]
    async fn reload_revokes_queued_cold_direct_udp_before_target_handshake_completes() {
        let dir=tempfile::tempdir().unwrap();
        let store=Arc::new(Store::open(dir.path().join("cold-revoke.sqlite").to_str().unwrap()).unwrap());
        store.setup("10.89.0.0/24","hub.example:51820",25).unwrap();
        store.add_group(&Group{id:"a".into(),name:"A".into(),allowed_groups:vec!["b".into()]}).unwrap();
        store.add_group(&Group{id:"b".into(),name:"B".into(),allowed_groups:vec![]}).unwrap();
        store.set_acl("a", &["b".into()]).unwrap();
        let a_secret=StaticSecret::from([91u8;32]); let b_secret=StaticSecret::from([92u8;32]);
        for (id,secret,ip,group) in [("a",&a_secret,"10.89.0.2","a"),("b",&b_secret,"10.89.0.3","b")] {
            store.add_peer(&Peer{id:id.into(),name:id.into(),public_key:base64::engine::general_purpose::STANDARD.encode(PublicKey::from(secret).as_bytes()),ipv4:ip.into(),group_id:group.into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None}).unwrap();
        }
        let hub_private=[93u8;32]; let hub_public=PublicKey::from(&StaticSecret::from(hub_private));
        let server=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let address=server.local_addr().unwrap();
        let (commands_tx,commands_rx)=mpsc::channel(2); let (ready,ready_rx)=oneshot::channel(); let stats=RuntimeStats::default();
        tokio::spawn(run_udp(server,store.clone(),hub_private,commands_rx,stats.clone(),Some(ready))); ready_rx.await.unwrap().unwrap();
        let a_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap(); let b_socket=UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut a=Tunn::new(a_secret.clone(),hub_public,None,None,91,None); let mut b=Tunn::new(b_secret.clone(),hub_public,None,None,92,None);
        let mut tx=vec![0;65535]; let mut rx=vec![0;65535];
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await; establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        a=Tunn::new(a_secret.clone(),hub_public,None,None,91,None);
        establish_client(&a_socket,address,&mut a,&mut tx,&mut rx).await;
        let request=service_packet(17,[10,89,0,2],[10,89,0,3],2345,9090,0,b"revoke-cold");
        send_inner(&a_socket,address,&mut a,&request,&mut tx).await;
        let (_n,_)=timeout(Duration::from_secs(3),b_socket.recv_from(&mut rx)).await.unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(rx[..4].try_into().unwrap()),1,"cold direct UDP is pending behind target handshake");

        // Revocation acknowledgement clears the pending application datagram.
        store.set_acl("a", &[]).unwrap();
        let (ack,wait)=oneshot::channel(); commands_tx.send(ReloadCommand{ack}).await.unwrap(); wait.await.unwrap().unwrap();
        b=Tunn::new(b_secret,hub_public,None,None,92,None);
        // Complete a fresh target handshake after the acknowledged revocation.
        // Any handshake initiation sent before reload was intentionally discarded.
        establish_client(&b_socket,address,&mut b,&mut tx,&mut rx).await;
        let TunnResult::WriteToNetwork(keepalive)=b.encapsulate(&[],&mut tx) else {panic!("expected authenticated keepalive")};
        b_socket.send_to(keepalive,address).await.unwrap();
        assert!(timeout(Duration::from_millis(250),b_socket.recv_from(&mut rx)).await.is_err(),"revoked pending direct datagram must not arrive after handshake");
        let snapshot=stats.read().await;
        assert_eq!(snapshot["a"].0,0,"queued packet was not accounted as delivered ingress");
        assert_eq!(snapshot["b"].1,0,"queued packet was not accounted as delivered egress");
    }

    #[test]
    fn expired_queued_reverse_reply_cannot_fall_back_to_acl_direct_route() {
        // Anchor synthetic flow time in the past so the queue deadline remains
        // live at the simulated 61-second flow-expiry check.
        let now=Instant::now()-Duration::from_secs(61);
        let a=Peer{id:"a".into(),name:"a".into(),public_key:String::new(),ipv4:"10.90.0.2".into(),group_id:"a".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let b=Peer{id:"b".into(),name:"b".into(),public_key:String::new(),ipv4:"10.90.0.3".into(),group_id:"b".into(),received_bytes:0,sent_bytes:0,last_handshake_unix:None};
        let ga=Group{id:"a".into(),name:"a".into(),allowed_groups:vec!["b".into()]};
        let gb=Group{id:"b".into(),name:"b".into(),allowed_groups:vec!["a".into()]};
        let mut peers=HashMap::new();
        for (peer,group,key) in [(a.clone(),ga.clone(),[101u8;32]),(b.clone(),gb.clone(),[102u8;32])] {
            let tunnel=Tunn::new(StaticSecret::from(key),PublicKey::from(&StaticSecret::from([103u8;32])),None,None,1,None);
            peers.insert(peer.id.clone(),RuntimePeer{peer,group:Some(group),tunnel,endpoint:None,last_data_unix:None});
        }
        let mut nat=Flows::new(Ipv4Addr::new(10,90,0,1),&[]);
        let original=ipv4::validate(&service_packet(17,[10,90,0,2],[10,90,0,3],1234,9090,0,b"request"),&a,Some(&ga)).unwrap();
        let (_bytes,reservation)=nat.prepare_direct(&original,&a,&peers["b"].peer,now).unwrap();
        nat.complete(reservation,true,now);
        let reply=ipv4::validate(&service_packet(17,[10,90,0,3],[10,90,0,2],9090,1234,0,b"reply"),&b,Some(&gb)).unwrap();
        // The independent ACL permits B->A direct traffic, so generic resolution
        // would produce a direct plan once the reverse-flow entry expires.
        assert!(resolve_packet(&reply,&b,&gb,"b",&peers,&[],Some(Ipv4Addr::new(10,90,0,1)),&mut nat,now+Duration::from_secs(61)).0.is_some());

        // Give this manually constructed queue item a longer deadline so the
        // synthetic timestamp isolates reverse-flow expiry from queue expiry.
        let queued=PendingDelivery{source_id:"b".into(),packet:reply,reply_only:true,deadline:now+Duration::from_secs(120)};
        let at=now+Duration::from_secs(61);
        assert!(!queued.expired_at(at),"test advances NAT expiry while keeping queue deadline live");
        assert!(nat.lookup_reply(&queued.packet,&b,at).is_none(),"reverse mapping has expired");
        // This is the drain_pending reply_only branch: it must only consult the
        // reverse mapping, never call resolve_packet and re-route by ACL.
        let queued_reply_plan=if queued.reply_only {
            nat.lookup_reply(&queued.packet,&b,at).map(|(target_id,bytes,reservation)|DeliveryPlan{source_id:queued.source_id.clone(),target_id,bytes,reservation:Some(reservation),udp:true})
        } else {
            resolve_packet(&queued.packet,&b,&gb,"b",&peers,&[],Some(Ipv4Addr::new(10,90,0,1)),&mut nat,at).0
        };
        assert!(queued_reply_plan.is_none(),"expired queued reverse reply cannot use an ACL-permitted direct route");
    }
}
