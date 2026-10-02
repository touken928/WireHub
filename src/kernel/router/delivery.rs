use super::*;

pub(super) struct DeliveryPlan { pub(super) source_id: String, pub(super) target_id: String, pub(super) bytes: Vec<u8>, pub(super) reservation: Option<Reservation>, pub(super) forward_id: Option<String> }

pub(super) struct PendingDelivery { pub(super) source_id: String, pub(super) source_key:String, pub(super) source_ip:Ipv4Addr, pub(super) packet: ipv4::ValidatedPacket, pub(super) reply_only: bool, pub(super) deadline: Instant, pub(super) target_id: String, pub(super) target_key:String, pub(super) target_ip:Ipv4Addr, pub(super) forward_id: Option<String>, pub(super) forward_protocol:Option<String>, pub(super) forward_target_port:Option<u16> }
impl PendingDelivery { pub(super) fn expired_at(&self, now: Instant) -> bool { self.deadline <= now } }

pub(super) fn enqueue_pending(queue: &mut VecDeque<PendingDelivery>, bytes: &mut usize, delivery: PendingDelivery) {
    let size = delivery.packet.bytes().len();
    if queue.len() >= PENDING_LIMIT || bytes.saturating_add(size) > PENDING_BYTES { return; }
    *bytes += size;
    queue.push_back(delivery);
}

pub(super) fn retain_pending(queue:&mut VecDeque<PendingDelivery>, bytes:&mut usize, peers:&HashMap<String,RuntimePeer>, forwards:&[Forward], hub:Option<Ipv4Addr>, nat:&Flows) {
    queue.retain(|item| {
        let source=peers.get(&item.source_id);let target=peers.get(&item.target_id);
        let identities=item.packet.src()==item.source_ip && source.is_some_and(|p|p.config.public_key==item.source_key && p.config.ip==item.source_ip) && target.is_some_and(|p|p.config.public_key==item.target_key && p.config.ip==item.target_ip);
        let route_allowed=if item.reply_only { source.is_some_and(|p|nat.has_reply_mapping(&item.packet,&p.config,Instant::now())) } else if let Some(id)=&item.forward_id { source.zip(target).is_some_and(|(s,t)|forwards.iter().any(|f|&f.id==id && Some(f.protocol.as_str())==item.forward_protocol.as_deref() && Some(f.target_port)==item.forward_target_port && Some(item.packet.dst())==hub && f.target_peer_id==item.target_id && policy::forward_allowed(f,&s.config.group_id,s.config.group.as_ref(),&t.config.group_id))) } else { source.zip(target).is_some_and(|(s,t)|policy::route_allowed(s.config.group.as_ref(),t.config.group.as_ref())) };
        let valid=identities && route_allowed;
        if !valid { *bytes=bytes.saturating_sub(item.packet.bytes().len()); }
        valid
    });
}

pub(super) fn resolve_packet(packet: &ipv4::ValidatedPacket, source: &impl crate::kernel::snapshot::PeerConfigView, source_group: &Group, source_id: &str, peers: &HashMap<String, RuntimePeer>, forwards: &[Forward], hub_ip: Option<Ipv4Addr>, nat: &mut Flows, now: Instant) -> (Option<DeliveryPlan>, bool) {
    resolve_packet_indexed(packet,source,source_group,source_id,peers,None,forwards,hub_ip,nat,now)
}

pub(super) fn resolve_packet_indexed(packet: &ipv4::ValidatedPacket, source: &impl crate::kernel::snapshot::PeerConfigView, source_group: &Group, source_id: &str, peers: &HashMap<String, RuntimePeer>, ips:Option<&HashMap<Ipv4Addr,String>>, forwards: &[Forward], hub_ip: Option<Ipv4Addr>, nat: &mut Flows, now: Instant) -> (Option<DeliveryPlan>, bool) {
    if let Some((target_id, bytes, reservation)) = nat.lookup_reply(packet, source, now) {
        return (Some(DeliveryPlan { source_id: source_id.into(), target_id, bytes, reservation: Some(reservation), forward_id:None }), true);
    }
    // An unassociated error must never fall back to an ACL route or a new flow.
    if matches!(packet.association(), FlowAssociation::Related { .. }) { return (None, true); }
    let mut terminal = false;
    let mut plan = None;
    if packet.dst() == hub_ip.unwrap_or(Ipv4Addr::UNSPECIFIED) {
        for forward in forwards {
            let Some(target) = peers.get(&forward.target_peer_id) else { continue };
            if !policy::forward_allowed(forward, source.group_id(), Some(source_group), &target.config.group_id) { continue; }
            if let Some((bytes, reservation)) = nat.prepare_forward_packet(packet, source, forward, &target.config, now) {
                terminal = true;
                plan = Some(DeliveryPlan { source_id: source_id.into(), target_id: target.config.id.clone(), bytes, reservation: Some(reservation), forward_id:Some(forward.id.clone()) });
                break;
            }
        }
    }
    if !terminal {
        let target=ips.and_then(|index|index.get(&packet.dst())).and_then(|id|peers.get(id)).or_else(||peers.values().find(|p|p.config.ip==packet.dst()));
        if let Some(target) = target {
                if policy::route_allowed(Some(source_group), target.config.group.as_ref()) {
                if matches!(packet.association(), FlowAssociation::Stateless) {
                    plan = Some(DeliveryPlan { source_id: source_id.into(), target_id: target.config.id.clone(), bytes: packet.bytes().to_vec(), reservation: None, forward_id:None });
                } else if let Some((bytes, reservation)) = nat.prepare_direct(packet, source, &target.config, now) {
                    plan = Some(DeliveryPlan { source_id: source_id.into(), target_id: target.config.id.clone(), bytes, reservation: Some(reservation), forward_id:None });
                }
            }
        }
    }
    (plan, false)
}

pub(super) async fn drain_pending(socket: &UdpSocket, queue: &mut VecDeque<PendingDelivery>, queued_bytes: &mut usize, peers: &mut HashMap<String, RuntimePeer>, forwards: &[Forward], hub_ip: Option<Ipv4Addr>, nat: &mut Flows, out: &mut [u8]) {
    let mut retry = VecDeque::new();
    while let Some(item) = queue.pop_front() {
        *queued_bytes = queued_bytes.saturating_sub(item.packet.bytes().len());
        // Each item gets a fresh clock reading: prior delivery awaits must not
        // let a later queued item outlive its deadline or reverse-flow expiry.
        let now = Instant::now();
        if item.expired_at(now) { continue; }
        let Some(source) = peers.get(&item.source_id) else { continue };
        let Some(group) = source.config.group.as_ref() else { continue };
        let source_peer = source.config.clone();
        let source_group = group.clone();
        let result = if item.reply_only {
            nat.lookup_reply(&item.packet, &source_peer, now).map(|(target_id, bytes, reservation)| DeliveryPlan { source_id: item.source_id.clone(), target_id, bytes, reservation: Some(reservation), forward_id:None })
        } else {
            resolve_packet(&item.packet, &source_peer, &source_group, &item.source_id, peers, forwards, hub_ip, nat, now).0
        };
        let Some(mut plan) = result else { nat.cancel_pending_packet(&item.packet,&source_peer); continue };
        let provenance_ok=plan.target_id==item.target_id && plan.forward_id==item.forward_id && peers.get(&item.source_id).is_some_and(|p|p.config.ip==item.source_ip) && peers.get(&item.target_id).is_some_and(|p|p.config.ip==item.target_ip) && item.forward_id.as_ref().map_or(true,|id|forwards.iter().any(|f|&f.id==id && Some(f.protocol.as_str())==item.forward_protocol.as_deref() && Some(f.target_port)==item.forward_target_port));
        if !provenance_ok { complete_delivery(nat,peers,&mut plan,EgressOutcome::Failed); nat.cancel_pending_packet(&item.packet,&source_peer); continue; }
        let outcome = deliver_plan(socket, peers, &plan, out).await;
        complete_delivery(nat, peers, &mut plan, outcome);
        if outcome == EgressOutcome::NotReady { retry.push_back(item); }
    }
    for item in retry { enqueue_pending(queue, queued_bytes, item); }
}

pub(super) fn complete_delivery(nat: &mut Flows, peers: &mut HashMap<String, RuntimePeer>, plan: &mut DeliveryPlan, outcome: EgressOutcome) {
    let delivered = outcome == EgressOutcome::Delivered;
    if let Some(reservation) = plan.reservation.take() { nat.complete(reservation, delivered, Instant::now()); }
    if delivered {
        let n = plan.bytes.len() as u64;
        if let Some(source) = peers.get_mut(&plan.source_id) { source.stats.received_bytes = source.stats.received_bytes.saturating_add(n); }
        if let Some(target) = peers.get_mut(&plan.target_id) { target.stats.sent_bytes = target.stats.sent_bytes.saturating_add(n); }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EgressOutcome { Delivered, Failed, NotReady }

pub(super) async fn publish_stats(peers: &HashMap<String, RuntimePeer>, stats: &RuntimeStats) {
    #[cfg(test)]
    STATS_PUBLISH_COUNT.try_with(|count| {
        count.set(count.get() + 1);
        STATS_PUBLISH_OBSERVER.try_with(|observer| { let _ = observer.send(count.get()); }).ok();
    }).ok();
    let mut snapshot = stats.write().await;
    snapshot.clear();
    snapshot.extend(peers.iter().map(|(id, p)| (id.clone(), (p.stats.received_bytes, p.stats.sent_bytes, p.stats.last_handshake_unix, p.stats.last_data_unix))));
}

pub(super) async fn publish_packet_stats(peers: &HashMap<String, RuntimePeer>, stats: &RuntimeStats, authenticated: bool) {
    if authenticated {
        publish_stats(peers, stats).await;
    }
}

pub(super) async fn deliver_plan(socket:&UdpSocket, peers:&mut HashMap<String,RuntimePeer>, plan:&DeliveryPlan, out:&mut [u8]) -> EgressOutcome {
    let Some(target) = peers.get_mut(&plan.target_id) else { return EgressOutcome::Failed };
    if target.session.tunnel.time_since_last_handshake().is_none() {
        // Cold delivery is not accepted: do not pass plaintext
        // into BoringTun's internal queue or commit a flow reservation.
        if let Some(endpoint) = target.session.endpoint {
            if let TunnResult::WriteToNetwork(initiation) = target.session.tunnel.format_handshake_initiation(out, false) {
                let _ = socket.send_to(initiation, endpoint).await;
            }
        }
        return EgressOutcome::NotReady;
    }
    let wire = match target.session.tunnel.encapsulate(&plan.bytes, out) {
        TunnResult::WriteToNetwork(wire) => wire,
        _ => return EgressOutcome::Failed,
    };
    if !matches!(Tunn::parse_incoming_packet(wire), Ok(Packet::PacketData(_))) {
        return EgressOutcome::NotReady;
    }
    let Some(endpoint) = target.session.endpoint else { return EgressOutcome::Failed };
    if socket.send_to(wire, endpoint).await.is_ok() { EgressOutcome::Delivered } else { EgressOutcome::Failed }
}

pub(super) fn unix_now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64 }
