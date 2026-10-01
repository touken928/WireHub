//! Bounded bidirectional state. Pending reservations never grant reply access.
use std::{collections::{HashMap, HashSet}, net::Ipv4Addr, time::{Duration, Instant}};

use crate::{transport::ipv4::{PacketTuple, ValidatedPacket}, model::{Forward, Peer}};
#[cfg(test)]
use crate::transport::ipv4;

const CAPACITY: usize = 16_384;
/// Maximum active and pending flows initiated by one peer, across TCP and UDP.
const PEER_CAPACITY: usize = 256;
const UDP_IDLE: Duration = Duration::from_secs(60);
const TCP_IDLE: Duration = Duration::from_secs(300);
const SNAT_START: u16 = 40_000;
const SNAT_END: u16 = 60_999;

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub(crate) struct Tuple {
    pub peer: String,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub frontend_ip: Ipv4Addr,
    pub frontend_port: u16,
    pub protocol: u8,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
struct Reverse { peer: String, proto: u8, src: Ipv4Addr, sport: u16, dst: Ipv4Addr, dport: u16 }

#[derive(Clone)]
struct Flow {
    reply: Reverse,
    output: Option<PacketTuple>,
    backend: String,
    backend_ip: Ipv4Addr,
    initiator_key: String,
    backend_key: String,
    forward_id: Option<String>,
    last: Instant,
}

/// A delivery must call `complete` exactly once. Failed delivery releases a new
/// reservation; only successful delivery installs or refreshes active state.
pub(crate) struct Reservation { key: Tuple, flow: Flow, is_new: bool }

pub(crate) struct Flows {
    flows: HashMap<Tuple, Flow>,
    reverse: HashMap<Reverse, Tuple>,
    pending: HashMap<Tuple, Flow>,
    pending_reverse: HashMap<Reverse, Tuple>,
    peer_counts: HashMap<String, usize>,
    hub_ip: Ipv4Addr,
    service_ports: HashSet<(u8, u16)>,
    next_udp_snat: u16,
    next_tcp_snat: u16,
}

impl Default for Flows { fn default() -> Self { Self::new(Ipv4Addr::new(10, 77, 0, 1), &[]) } }

impl Flows {
    #[cfg(test)]
    pub(crate) fn test_state_counts(&self) -> (usize, usize) { (self.flows.len(), self.pending.len()) }

    pub fn new(hub_ip: Ipv4Addr, forwards: &[Forward]) -> Self {
        let service_ports = forwards.iter().filter_map(|f| protocol(&f.protocol).map(|p| (p, f.target_port))).collect();
        Self { flows: HashMap::new(), reverse: HashMap::new(), pending: HashMap::new(), pending_reverse: HashMap::new(), peer_counts: HashMap::new(), hub_ip, service_ports, next_udp_snat: SNAT_START, next_tcp_snat: SNAT_START }
    }

    pub fn clear(&mut self) {
        self.flows.clear(); self.reverse.clear(); self.pending.clear(); self.pending_reverse.clear(); self.peer_counts.clear();
    }

    pub(crate) fn reconcile(&mut self, old_hub: Option<Ipv4Addr>, new_hub: Option<Ipv4Addr>, old_forwards: &[Forward], new_forwards: &[Forward], peers: &HashMap<String, crate::transport::RuntimePeer>) {
        let same_hub = old_hub == new_hub;
        self.flows.retain(|key, flow| {
            if !same_hub { return false; }
            let Some(source) = peers.get(&key.peer) else { return false };
            let Some(target) = peers.get(&flow.backend) else { return false };
            if source.peer.public_key != flow.initiator_key || target.peer.public_key != flow.backend_key || source.peer.ipv4.parse::<Ipv4Addr>().ok() != Some(key.ip) || target.peer.ipv4.parse::<Ipv4Addr>().ok() != Some(flow.backend_ip) { return false; }
            if flow.forward_id.is_some() {
                let Some(id) = flow.forward_id.as_deref() else { return false };
                let Some(before) = old_forwards.iter().find(|f| f.id == id) else { return false };
                let Some(after) = new_forwards.iter().find(|f| f.id == id) else { return false };
                if before.protocol != after.protocol || before.target_peer_id != after.target_peer_id || before.target_port != after.target_port || !after.allowed_group_ids.contains(&source.peer.group_id) || !crate::policy::forward_allowed(after, &source.peer.group_id, source.group.as_ref(), &target.peer.group_id) { return false; }
            } else if !crate::policy::allows(source.group.as_ref().unwrap_or(&crate::model::Group {id:String::new(),name:String::new(),allowed_groups:vec![]}), target.group.as_ref().unwrap_or(&crate::model::Group {id:String::new(),name:String::new(),allowed_groups:vec![]})) { return false; }
            true
        });
        let service_ports: HashSet<_> = new_forwards.iter().filter_map(|f| protocol(&f.protocol).map(|p| (p, f.target_port))).collect();
        self.remove_service_port_collisions(&service_ports);
        self.reverse.retain(|_,key| self.flows.contains_key(key));
        self.pending.clear(); self.pending_reverse.clear();
        self.rebuild_peer_counts();
        self.service_ports = service_ports;
        self.hub_ip = new_hub.unwrap_or(Ipv4Addr::UNSPECIFIED);
    }

    fn remove_service_port_collisions(&mut self, service_ports: &HashSet<(u8, u16)>) {
        self.flows.retain(|_, flow| {
            !flow.output.as_ref().is_some_and(|output| service_ports.contains(&(flow.reply.proto, output.src_port)))
        });
        self.reverse.retain(|_, key| self.flows.contains_key(key));
        self.rebuild_peer_counts();
    }

    /// Reclaim expired records. Called at bounded sweep intervals and before capacity allocation.
    pub fn expire(&mut self, now: Instant) {
        self.flows.retain(|key, flow| now.saturating_duration_since(flow.last) < idle(key.protocol));
        self.reverse.retain(|_, key| self.flows.contains_key(key));
        self.pending.retain(|key, flow| now.saturating_duration_since(flow.last) < idle(key.protocol));
        self.pending_reverse.retain(|_, key| self.pending.contains_key(key));
        self.rebuild_peer_counts();
    }

    /// Prepare ordinary direct UDP delivery. `destination` identifies the peer
    /// that owns the packet's destination IP; reverse access is keyed to it.
    pub(crate) fn prepare_direct(&mut self, packet: &ValidatedPacket, source: &Peer, destination: &Peer, now: Instant) -> Option<(Vec<u8>, Reservation)> {
        let (sport, dport) = (packet.src_port()?, packet.dst_port()?);
        let proto = packet.protocol();
        if !matches!(proto, 6 | 17) || packet.dst().to_string() != destination.ipv4 { return None; }
        let key = Tuple { peer: source.id.clone(), ip: packet.src(), port: sport, frontend_ip: packet.dst(), frontend_port: dport, protocol: proto };
        if self.flows.get(&key).is_some_and(|f| now.saturating_duration_since(f.last)>=idle(proto)) { self.remove_active(&key); }
        if proto == 6 && !self.flows.contains_key(&key) && !packet.tcp_flags().is_some_and(|flags| flags & 2 != 0 && flags & 16 == 0) { return None; }
        self.prepare_common(packet, key, destination, None, now, None, source.public_key.clone())
    }

    /// Prepare translated forwarding while retaining the frontend tuple for replies.
    /// Existing live flows accept any valid TCP segment; a new TCP record requires SYN without ACK.
    pub(crate) fn prepare_forward_packet(&mut self, packet: &ValidatedPacket, source: &Peer, forward: &Forward, backend: &Peer, now: Instant) -> Option<(Vec<u8>, Reservation)> {
        let proto = protocol(&forward.protocol)?;
        if packet.protocol() != proto || packet.dst() != self.hub_ip || packet.dst_port()? != forward.target_port { return None; }
        let backend_ip: Ipv4Addr = backend.ipv4.parse().ok()?;
        let key = Tuple { peer: source.id.clone(), ip: packet.src(), port: packet.src_port()?, frontend_ip: packet.dst(), frontend_port: packet.dst_port()?, protocol: proto };

        // Do expiry before deciding whether this is established: an expired tuple
        // is a new flow and therefore must pass the TCP initial-SYN rule.
        if self.flows.get(&key).is_some_and(|f| now.saturating_duration_since(f.last) >= idle(proto)) {
            self.remove_active(&key);
        }
        let active = self.flows.get(&key).cloned();
        let flow = if let Some(flow) = active {
            if flow.backend != backend.id || flow.backend_ip != backend_ip { return None; }
            flow
        } else if self.pending.contains_key(&key) {
            // Pending reservations are exclusive; do not hand out a second token
            // that could release the first owner's reservation on failure.
            return None;
        } else {
            if proto == 6 && !packet.tcp_flags().is_some_and(|flags| flags & 2 != 0 && flags & 16 == 0) { return None; }
            self.reserve_capacity(&key.peer, now)?;
            let snat = self.choose_snat(proto, backend, forward.target_port)?;
            let reply = Reverse { peer: backend.id.clone(), proto, src: backend_ip, sport: forward.target_port, dst: self.hub_ip, dport: snat };
            let flow = Flow { reply: reply.clone(), output: Some(PacketTuple { src: self.hub_ip, src_port: snat, dst: backend_ip, dst_port: forward.target_port }), backend: backend.id.clone(), backend_ip, initiator_key:source.public_key.clone(), backend_key:backend.public_key.clone(), forward_id:Some(forward.id.clone()), last: now };
            self.pending_reverse.insert(reply, key.clone());
            self.pending.insert(key.clone(), flow.clone());
            self.add_peer_count(&key.peer);
            flow
        };
        let is_new = !self.flows.contains_key(&key);
        let output = packet.clone().emit(flow.output);
        Some((output, Reservation { key, flow, is_new }))
    }

    fn prepare_common(&mut self, packet: &ValidatedPacket, key: Tuple, destination: &Peer, output: Option<PacketTuple>, now: Instant, forward_id: Option<String>, initiator_key: String) -> Option<(Vec<u8>, Reservation)> {
        if self.flows.get(&key).is_some_and(|f| now.saturating_duration_since(f.last) >= idle(key.protocol)) {
            self.remove_active(&key);
        }
        let destination_ip: Ipv4Addr = destination.ipv4.parse().ok()?;
        let flow = if let Some(flow) = self.flows.get(&key).cloned() {
            if flow.backend != destination.id || flow.backend_ip != destination_ip { return None; }
            flow
        } else if self.pending.contains_key(&key) {
            return None;
        } else {
            self.reserve_capacity(&key.peer, now)?;
            let reply = Reverse { peer: destination.id.clone(), proto: key.protocol, src: key.frontend_ip, sport: key.frontend_port, dst: key.ip, dport: key.port };
            let flow = Flow { reply: reply.clone(), output, backend: destination.id.clone(), backend_ip: destination_ip, initiator_key, backend_key:destination.public_key.clone(), forward_id, last: now };
            self.pending_reverse.insert(reply, key.clone());
            self.pending.insert(key.clone(), flow.clone());
            self.add_peer_count(&key.peer);
            flow
        };
        let is_new = !self.flows.contains_key(&key);
        Some((packet.clone().emit(flow.output), Reservation { key, flow, is_new }))
    }

    fn reserve_capacity(&mut self, peer: &str, now: Instant) -> Option<()> {
        // Sweep at the relevant boundary too: a peer can be full long before
        // the global table is, and expired records must not strand its quota.
        if self.flows.len() + self.pending.len() >= CAPACITY
            || self.peer_counts.get(peer).copied().unwrap_or(0) >= PEER_CAPACITY
        {
            self.expire(now);
        }
        (self.flows.len() + self.pending.len() < CAPACITY
            && self.peer_counts.get(peer).copied().unwrap_or(0) < PEER_CAPACITY).then_some(())
    }

    fn choose_snat(&mut self, proto: u8, backend: &Peer, target_port: u16) -> Option<u16> {
        let backend_ip = backend.ipv4.parse().ok()?;
        let slots = (SNAT_END - SNAT_START + 1) as usize;
        let counter = if proto == 6 { &mut self.next_tcp_snat } else { &mut self.next_udp_snat };
        for _ in 0..slots {
            let candidate = *counter;
            *counter = if candidate == SNAT_END { SNAT_START } else { candidate + 1 };
            let idx = Reverse { peer: backend.id.clone(), proto, src: backend_ip, sport: target_port, dst: self.hub_ip, dport: candidate };
            if !self.service_ports.contains(&(proto, candidate)) && !self.reverse.contains_key(&idx) && !self.pending_reverse.contains_key(&idx) { return Some(candidate); }
        }
        None
    }

    /// Commit/refresh only after successful business-data delivery; failure only releases pending state.
    pub fn complete(&mut self, reservation: Reservation, delivered: bool, now: Instant) {
        if !delivered {
            if reservation.is_new && self.pending.get(&reservation.key).is_some_and(|f| f.reply == reservation.flow.reply && f.last == reservation.flow.last) { self.remove_pending(&reservation.key); }
            return;
        }
        // Expiry, clear, or reconciliation may have invalidated a queued
        // reservation while delivery was outstanding. Do not resurrect it.
        if reservation.is_new
            && !self.pending.get(&reservation.key).is_some_and(|f| f.reply == reservation.flow.reply && f.last == reservation.flow.last)
        {
            return;
        }
        let mut flow = reservation.flow;
        flow.last = now;
        self.detach_pending(&reservation.key);
        self.reverse.insert(flow.reply.clone(), reservation.key.clone());
        self.flows.insert(reservation.key, flow);
    }

    fn remove_pending(&mut self, key: &Tuple) {
        if self.pending.remove(key).is_some() { self.remove_peer_count(&key.peer); }
        self.pending_reverse.retain(|_, value| value != key);
    }

    fn detach_pending(&mut self, key: &Tuple) {
        self.pending.remove(key);
        self.pending_reverse.retain(|_, value| value != key);
    }

    /// Release an orphaned reservation belonging to a queued packet whose
    /// current route no longer matches its captured provenance.
    pub(crate) fn cancel_pending_packet(&mut self, packet: &ValidatedPacket, source: &Peer) {
        let (Some(port),Some(frontend_port))=(packet.src_port(),packet.dst_port()) else { return };
        let key=Tuple{peer:source.id.clone(),ip:packet.src(),port,frontend_ip:packet.dst(),frontend_port,protocol:packet.protocol()};
        self.remove_pending(&key);
    }

    fn remove_active(&mut self, key: &Tuple) {
        if let Some(flow) = self.flows.remove(key) { self.reverse.remove(&flow.reply); self.remove_peer_count(&key.peer); }
    }

    fn add_peer_count(&mut self, peer: &str) { *self.peer_counts.entry(peer.to_owned()).or_default() += 1; }

    fn remove_peer_count(&mut self, peer: &str) {
        if let Some(count) = self.peer_counts.get_mut(peer) {
            *count = count.saturating_sub(1);
            if *count == 0 { self.peer_counts.remove(peer); }
        }
    }

    fn rebuild_peer_counts(&mut self) {
        self.peer_counts.clear();
        for key in self.flows.keys().chain(self.pending.keys()) { *self.peer_counts.entry(key.peer.clone()).or_default() += 1; }
    }

    /// A live reverse hit is a normal delivery reservation and must be completed
    /// with its delivery result to refresh the flow's idle timer.
    pub(crate) fn lookup_reply(&mut self, packet: &ValidatedPacket, peer: &Peer, now: Instant) -> Option<(String, Vec<u8>, Reservation)> {
        if packet.protocol()==6 && packet.tcp_flags().is_some_and(|flags| flags & 2 != 0 && flags & 16 == 0) { return None; }
        let tuple = Reverse { peer: peer.id.clone(), proto: packet.protocol(), src: packet.src(), sport: packet.src_port()?, dst: packet.dst(), dport: packet.dst_port()? };
        let key = self.reverse.get(&tuple)?.clone();
        let flow = self.flows.get(&key)?.clone();
        if now.saturating_duration_since(flow.last) >= idle(key.protocol) { self.remove_active(&key); return None; }
        let output = PacketTuple { src: key.frontend_ip, src_port: key.frontend_port, dst: key.ip, dst_port: key.port };
        let packet = packet.clone().emit(Some(output));
        let target_peer = key.peer.clone();
        Some((target_peer, packet, Reservation { key, flow, is_new: false }))
    }

    pub(crate) fn has_reply_mapping(&self, packet:&ValidatedPacket, peer:&Peer, now:Instant)->bool {
        let Some(sport)=packet.src_port() else{return false};let Some(dport)=packet.dst_port() else{return false};
        let tuple=Reverse{peer:peer.id.clone(),proto:packet.protocol(),src:packet.src(),sport,dst:packet.dst(),dport};
        self.reverse.get(&tuple).and_then(|key|self.flows.get(key)).is_some_and(|flow|now.saturating_duration_since(flow.last)<idle(packet.protocol()))
    }
}

fn idle(proto: u8) -> Duration { if proto == 17 { UDP_IDLE } else { TCP_IDLE } }
fn protocol(value: &str) -> Option<u8> { match value { "tcp" => Some(6), "udp" => Some(17), _ => None } }

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, ip: &str) -> Peer {
        Peer { id: id.into(), name: id.into(), public_key: String::new(), ipv4: ip.into(), group_id: String::new(), received_bytes: 0, sent_bytes: 0, last_handshake_unix: None }
    }
    fn forward(proto: &str, port: u16) -> Forward {
        Forward { id: "f".into(), name: "service".into(), protocol: proto.into(), target_peer_id: "backend".into(), target_port: port, allowed_group_ids: vec![] }
    }
    fn packet(proto: u8, src: Ipv4Addr, sport: u16, dst: Ipv4Addr, dport: u16, flags: u8) -> ValidatedPacket {
        let transport_len = if proto == 6 { 20 } else { 8 };
        let mut bytes = vec![0; 20 + transport_len];
        bytes[0] = 0x45;
        let total_len = bytes.len() as u16;
        bytes[2..4].copy_from_slice(&total_len.to_be_bytes());
        bytes[8] = 64;
        bytes[9] = proto;
        bytes[12..16].copy_from_slice(&src.octets());
        bytes[16..20].copy_from_slice(&dst.octets());
        bytes[20..22].copy_from_slice(&sport.to_be_bytes());
        bytes[22..24].copy_from_slice(&dport.to_be_bytes());
        if proto == 6 { bytes[32] = 0x50; bytes[33] = flags; }
        else { bytes[24..26].copy_from_slice(&8u16.to_be_bytes()); }
        if proto == 6 {
            let c = tcp_checksum(src.octets(), dst.octets(), &bytes[20..]);
            bytes[36..38].copy_from_slice(&c.to_be_bytes());
        }
        let checksum = ipv4::checksum(&bytes[..20]);
        bytes[10..12].copy_from_slice(&checksum.to_be_bytes());
        ipv4::validate_forwarded(&bytes).unwrap()
    }
    fn tcp_checksum(src: [u8; 4], dst: [u8; 4], segment: &[u8]) -> u16 {
        let mut pseudo = Vec::new();
        pseudo.extend_from_slice(&src); pseudo.extend_from_slice(&dst);
        pseudo.extend_from_slice(&[0, 6]); pseudo.extend_from_slice(&(segment.len() as u16).to_be_bytes());
        pseudo.extend_from_slice(segment);
        let mut sum = 0u32;
        for c in pseudo.chunks_exact(2) { sum += u16::from_be_bytes([c[0], c[1]]) as u32; }
        if pseudo.len() % 2 != 0 { sum += (pseudo[pseudo.len()-1] as u32) << 8; }
        while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
        !(sum as u16)
    }
    fn t() -> Instant { Instant::now() }

    #[test]
    fn direct_flow_uses_destination_identity_and_exact_reverse_tuple() {
        let mut flows = Flows::default();
        let src = peer("source", "10.77.0.2");
        let dst = peer("destination", "10.77.0.3");
        let now = t();
        let p = packet(17, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.3".parse().unwrap(), 5678, 0);
        let (_, r) = flows.prepare_direct(&p, &src, &dst, now).unwrap();
        assert!(flows.lookup_reply(&packet(17, "10.77.0.3".parse().unwrap(), 5678, "10.77.0.2".parse().unwrap(), 1234, 0), &dst, now).is_none());
        flows.complete(r, true, now);
        let (target, _, r) = flows.lookup_reply(&packet(17, "10.77.0.3".parse().unwrap(), 5678, "10.77.0.2".parse().unwrap(), 1234, 0), &dst, now).unwrap();
        assert_eq!(target, "source");
        flows.complete(r, true, now + Duration::from_secs(10));
        for altered in [
            ("wrong-peer", "10.77.0.3", 5678, "10.77.0.2", 1234),
            ("destination", "10.77.0.4", 5678, "10.77.0.2", 1234),
            ("destination", "10.77.0.3", 5679, "10.77.0.2", 1234),
            ("destination", "10.77.0.3", 5678, "10.77.0.2", 1235),
        ] {
            let peer = peer(altered.0, if altered.0 == "destination" { "10.77.0.3" } else { "10.77.0.9" });
            let p = packet(17, altered.1.parse().unwrap(), altered.2, altered.3.parse().unwrap(), altered.4, 0);
            assert!(flows.lookup_reply(&p, &peer, now + Duration::from_secs(11)).is_none());
        }
        assert!(flows.prepare_direct(&packet(6, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.3".parse().unwrap(), 5678, 2), &src, &dst, now).is_some(),"direct TCP SYN can initiate a tracked flow");
    }

    #[test]
    fn direct_tcp_one_way_acl_tracks_reply_but_denies_reverse_syn() {
        let mut flows = Flows::default();
        let source = peer("source", "10.77.0.2");
        let destination = peer("destination", "10.77.0.3");
        let now = t();
        let syn = packet(6, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.3".parse().unwrap(), 443, 0x02);
        let (_, reservation) = flows.prepare_direct(&syn, &source, &destination, now)
            .expect("one-way ACL permits initiating direct TCP SYN");
        flows.complete(reservation, true, now);

        for flags in [0x12, 0x10] { // SYN/ACK reply, then continuation ACK
            let reply = packet(6, "10.77.0.3".parse().unwrap(), 443, "10.77.0.2".parse().unwrap(), 1234, flags);
            let (target, _, reservation) = flows.lookup_reply(&reply, &destination, now).expect("exact reverse TCP tuple is authorized");
            assert_eq!(target, "source");
            flows.complete(reservation, true, now);
        }
        let reverse_syn = packet(6, "10.77.0.3".parse().unwrap(), 443, "10.77.0.2".parse().unwrap(), 1234, 0x02);
        assert!(flows.lookup_reply(&reverse_syn, &destination, now).is_none(), "reverse bare SYN is a new initiation, not a reply");
        let wrong_tuple = packet(6, "10.77.0.3".parse().unwrap(), 444, "10.77.0.2".parse().unwrap(), 1234, 0x12);
        assert!(flows.lookup_reply(&wrong_tuple, &destination, now).is_none());
    }

    #[test]
    fn forward_translation_identity_and_reverse_success_refresh_boundary() {
        let mut flows = Flows::default();
        let source = peer("source", "10.77.0.2");
        let backend = peer("backend", "10.77.0.3");
        let f = forward("udp", 9000);
        let now = t();
        let p = packet(17, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.1".parse().unwrap(), 9000, 0);
        let (translated, r) = flows.prepare_forward_packet(&p, &source, &f, &backend, now).unwrap();
        assert_eq!(&translated[12..16], &[10,77,0,1]);
        assert_eq!(u16::from_be_bytes([translated[22], translated[23]]), 9000);
        let snat = u16::from_be_bytes([translated[20], translated[21]]);
        flows.complete(r, true, now);
        let reply = packet(17, "10.77.0.3".parse().unwrap(), 9000, "10.77.0.1".parse().unwrap(), snat, 0);
        for (peer, src, sport, dst, dport) in [
            (&backend, "10.77.0.4", 9000, "10.77.0.1", snat),
            (&backend, "10.77.0.3", 9001, "10.77.0.1", snat),
            (&backend, "10.77.0.3", 9000, "10.77.0.2", snat),
            (&backend, "10.77.0.3", 9000, "10.77.0.1", snat + 1),
            (&source, "10.77.0.3", 9000, "10.77.0.1", snat),
        ] {
            let p = packet(17, src.parse().unwrap(), sport, dst.parse().unwrap(), dport, 0);
            assert!(flows.lookup_reply(&p, peer, now).is_none());
        }
        let (target, rewritten, r) = flows.lookup_reply(&reply, &backend, now + Duration::from_secs(59)).unwrap();
        assert_eq!(target, "source");
        assert_eq!(&rewritten[12..16], &[10,77,0,1]);
        assert_eq!(u16::from_be_bytes([rewritten[22], rewritten[23]]), 1234);
        flows.complete(r, true, now + Duration::from_secs(59));
        assert!(flows.lookup_reply(&reply, &backend, now + Duration::from_secs(118)).is_some());
        assert!(flows.lookup_reply(&reply, &backend, now + Duration::from_secs(119)).is_none());
        assert!(flows.reverse.is_empty());
    }

    #[test]
    fn failed_initial_delivery_does_not_grant_reply_access_or_refresh_active_flow() {
        let mut flows = Flows::default();
        let source = peer("source", "10.77.0.2"); let backend = peer("backend", "10.77.0.3");
        let now = t(); let f = forward("udp", 9000);
        let p = packet(17, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.1".parse().unwrap(), 9000, 0);
        let (_, r) = flows.prepare_forward_packet(&p, &source, &f, &backend, now).unwrap();
        assert!(flows.lookup_reply(&packet(17, "10.77.0.3".parse().unwrap(), 9000, "10.77.0.1".parse().unwrap(), 40000, 0), &backend, now).is_none());
        flows.complete(r, false, now);
        assert!(flows.pending.is_empty() && flows.pending_reverse.is_empty() && flows.reverse.is_empty());

        let (_, r) = flows.prepare_forward_packet(&p, &source, &f, &backend, now).unwrap();
        flows.complete(r, true, now);
        let (_, _, r) = flows.lookup_reply(&packet(17, "10.77.0.3".parse().unwrap(), 9000, "10.77.0.1".parse().unwrap(), 40001, 0), &backend, now + Duration::from_secs(30)).unwrap();
        flows.complete(r, false, now + Duration::from_secs(30));
        assert!(flows.lookup_reply(&packet(17, "10.77.0.3".parse().unwrap(), 9000, "10.77.0.1".parse().unwrap(), 40001, 0), &backend, now + Duration::from_secs(60)).is_none());
    }

    #[test]
    fn expired_forward_reclaimed_and_tcp_requires_syn_only_for_new_record() {
        let mut flows = Flows::default();
        let source = peer("source", "10.77.0.2"); let backend = peer("backend", "10.77.0.3"); let f = forward("tcp", 443);
        let now = t();
        let syn = packet(6, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.1".parse().unwrap(), 443, 2);
        let (_, r) = flows.prepare_forward_packet(&syn, &source, &f, &backend, now).unwrap(); flows.complete(r, true, now);
        let ack = packet(6, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.1".parse().unwrap(), 443, 16);
        assert!(flows.prepare_forward_packet(&ack, &source, &f, &backend, now + Duration::from_secs(299)).is_some());
        // Complete that existing delivery so its reservation does not affect pending state.
        let (_, r) = flows.prepare_forward_packet(&ack, &source, &f, &backend, now + Duration::from_secs(299)).unwrap(); flows.complete(r, true, now + Duration::from_secs(299));
        assert!(flows.prepare_forward_packet(&ack, &source, &f, &backend, now + Duration::from_secs(600)).is_none());
        assert!(flows.flows.is_empty() && flows.reverse.is_empty());
        assert!(flows.prepare_forward_packet(&syn, &source, &f, &backend, now + Duration::from_secs(600)).is_some());
    }

    #[test]
    fn pending_dedup_service_exclusion_protocol_sharing_and_capacity() {
        let backend = peer("backend", "10.77.0.3"); let source = peer("source", "10.77.0.2");
        let mut flows = Flows::new("10.77.0.1".parse().unwrap(), &[forward("udp", 40000)]);
        let now = t(); let udp_forward = forward("udp", 53);
        let udp = packet(17, "10.77.0.2".parse().unwrap(), 1111, "10.77.0.1".parse().unwrap(), 53, 0);
        let (first, reservation) = flows.prepare_forward_packet(&udp, &source, &udp_forward, &backend, now).unwrap();
        assert_eq!(u16::from_be_bytes([first[20], first[21]]), 40001);
        assert!(flows.prepare_forward_packet(&udp, &source, &udp_forward, &backend, now).is_none());
        // Identical numeric port may be used independently by TCP.
        let tcp_forward = forward("tcp", 53);
        let tcp = packet(6, "10.77.0.2".parse().unwrap(), 1111, "10.77.0.1".parse().unwrap(), 53, 2);
        let (translated_tcp, tcp_reservation) = flows.prepare_forward_packet(&tcp, &source, &tcp_forward, &backend, now).unwrap();
        assert_eq!(u16::from_be_bytes([translated_tcp[20], translated_tcp[21]]), 40000);
        flows.complete(reservation, false, now);
        flows.complete(tcp_reservation, true, now);

        let mut full = Flows::default();
        for n in 0..CAPACITY {
            let key = Tuple { peer: format!("p{n}"), ip: Ipv4Addr::LOCALHOST, port: n as u16, frontend_ip: Ipv4Addr::LOCALHOST, frontend_port: 9, protocol: 17 };
            let flow = Flow { reply: Reverse { peer: format!("b{n}"), proto: 17, src: Ipv4Addr::LOCALHOST, sport: 9, dst: Ipv4Addr::LOCALHOST, dport: n as u16 }, output: None, backend: "backend".into(), backend_ip: Ipv4Addr::LOCALHOST, initiator_key:String::new(),backend_key:String::new(),forward_id:None,last: now };
            full.pending_reverse.insert(flow.reply.clone(), key.clone()); full.pending.insert(key, flow);
        }
        assert!(full.reserve_capacity("capacity-check", now).is_none());

        let mut exhausted = Flows::default();
        for port in SNAT_START..=SNAT_END {
            exhausted.pending_reverse.insert(Reverse { peer: backend.id.clone(), proto: 17, src: backend.ipv4.parse().unwrap(), sport: 53, dst: exhausted.hub_ip, dport: port }, Tuple { peer: format!("used{port}"), ip: Ipv4Addr::LOCALHOST, port, frontend_ip: Ipv4Addr::LOCALHOST, frontend_port: 53, protocol: 17 });
        }
        assert!(exhausted.choose_snat(17, &backend, 53).is_none());
    }

    #[test]
    fn per_initiator_quota_is_shared_by_protocols_and_released_on_cleanup() {
        let backend = peer("backend", "10.77.0.3");
        let source = peer("source", "10.77.0.2");
        let other = peer("other", "10.77.0.4");
        let udp_forward = forward("udp", 53);
        let tcp_forward = forward("tcp", 443);
        let now = t();
        let mut flows = Flows::default();

        // Active UDP and pending TCP both count; distinct source ports cannot
        // evade the per-initiator limit, and a different initiator is unaffected.
        for port in 0..(PEER_CAPACITY / 2) {
            let p = packet(17, source.ipv4.parse().unwrap(), port as u16, flows.hub_ip, 53, 0);
            let (_, r) = flows.prepare_forward_packet(&p, &source, &udp_forward, &backend, now).unwrap();
            flows.complete(r, true, now);
        }
        for port in (PEER_CAPACITY / 2)..PEER_CAPACITY {
            let p = packet(6, source.ipv4.parse().unwrap(), port as u16, flows.hub_ip, 443, 2);
            assert!(flows.prepare_forward_packet(&p, &source, &tcp_forward, &backend, now).is_some());
        }
        let denied = packet(17, source.ipv4.parse().unwrap(), 10_000, flows.hub_ip, 53, 0);
        assert!(flows.prepare_forward_packet(&denied, &source, &udp_forward, &backend, now).is_none());
        let allowed = packet(17, other.ipv4.parse().unwrap(), 10_000, flows.hub_ip, 53, 0);
        assert!(flows.prepare_forward_packet(&allowed, &other, &udp_forward, &backend, now).is_some());

        // Repeated traffic on an already-active key refreshes without quota.
        let existing = packet(17, source.ipv4.parse().unwrap(), 0, flows.hub_ip, 53, 0);
        assert!(flows.prepare_forward_packet(&existing, &source, &udp_forward, &backend, now).is_some());

        // Failed pending delivery releases a slot immediately.
        let pending_key = Tuple { peer: source.id.clone(), ip: source.ipv4.parse().unwrap(), port: (PEER_CAPACITY / 2) as u16, frontend_ip: flows.hub_ip, frontend_port: 443, protocol: 6 };
        let reservation = flows.pending.get(&pending_key).cloned().unwrap();
        flows.complete(Reservation { key: pending_key, flow: reservation, is_new: true }, false, now);
        let replacement = packet(6, source.ipv4.parse().unwrap(), PEER_CAPACITY as u16, flows.hub_ip, 443, 2);
        assert!(flows.prepare_forward_packet(&replacement, &source, &tcp_forward, &backend, now).is_some());

        // Expiration, reconcile, and clear all restore admission capacity.
        flows.expire(now + Duration::from_secs(301));
        let after_expiry = packet(17, source.ipv4.parse().unwrap(), 11_000, flows.hub_ip, 53, 0);
        assert!(flows.prepare_forward_packet(&after_expiry, &source, &udp_forward, &backend, now + Duration::from_secs(301)).is_some());
        flows.reconcile(Some(flows.hub_ip), Some(flows.hub_ip), &[], &[], &HashMap::new());
        assert!(flows.flows.is_empty() && flows.pending.is_empty());
        assert!(flows.prepare_forward_packet(&after_expiry, &source, &udp_forward, &backend, now + Duration::from_secs(302)).is_some());
        flows.clear();
        assert!(flows.prepare_forward_packet(&after_expiry, &source, &udp_forward, &backend, now + Duration::from_secs(303)).is_some());
    }

    #[test]
    fn direct_quota_sweeps_peer_expiry_and_ignores_late_reservations() {
        let source = peer("source", "10.77.0.2");
        let destination = peer("destination", "10.77.0.3");
        let now = t();
        let mut flows = Flows::default();
        let mut last_reservation = None;

        for port in 0..PEER_CAPACITY {
            let p = packet(17, source.ipv4.parse().unwrap(), port as u16,
                destination.ipv4.parse().unwrap(), 9000, 0);
            let (_, reservation) = flows.prepare_direct(&p, &source, &destination, now).unwrap();
            if port + 1 == PEER_CAPACITY { last_reservation = Some(reservation); }
            else { flows.complete(reservation, true, now); }
        }
        assert_eq!(flows.peer_counts.get(&source.id), Some(&PEER_CAPACITY));

        // Pending state also consumes quota. Expire it at the per-peer
        // boundary while the global table is nowhere near full.
        let pending_packet = packet(17, source.ipv4.parse().unwrap(), 1000,
            destination.ipv4.parse().unwrap(), 9000, 0);
        assert!(flows.prepare_direct(&pending_packet, &source, &destination, now).is_none());
        flows.expire(now + UDP_IDLE);
        let reservation = last_reservation.unwrap();
        flows.complete(reservation, true, now + UDP_IDLE);
        assert!(flows.flows.is_empty());
        assert!(!flows.peer_counts.contains_key(&source.id));

        // Admission itself performs the same targeted cleanup; no separate
        // periodic global sweep is required for a full peer.
        let p = packet(17, source.ipv4.parse().unwrap(), 1001,
            destination.ipv4.parse().unwrap(), 9000, 0);
        assert!(flows.prepare_direct(&p, &source, &destination, now + UDP_IDLE).is_some());
    }

    #[test]
    fn service_port_collision_removes_only_matching_protocol_snat_mapping() {
        let mut flows = Flows::default();
        let source = "source".to_string();
        let peer_ip = Ipv4Addr::new(10, 77, 0, 2);
        let hub_ip = flows.hub_ip;
        for (port, proto, snat) in [(1234, 17, 40000), (1235, 17, 40001), (1236, 6, 40000)] {
            let key = Tuple { peer: source.clone(), ip: peer_ip, port, frontend_ip: hub_ip, frontend_port: 9000, protocol: proto };
            let reply = Reverse { peer: "backend".into(), proto, src: "10.77.0.3".parse().unwrap(), sport: 9000, dst: hub_ip, dport: snat };
            let flow = Flow { reply: reply.clone(), output: Some(PacketTuple { src: hub_ip, src_port: snat, dst: reply.src, dst_port: 9000 }), backend: "backend".into(), backend_ip: reply.src, initiator_key: String::new(), backend_key: String::new(), forward_id: Some("f".into()), last: t() };
            flows.reverse.insert(reply, key.clone());
            flows.flows.insert(key, flow);
        }
        flows.rebuild_peer_counts();
        assert_eq!(flows.peer_counts.get(&source), Some(&3));
        let service_ports = HashSet::from([(17, 40000)]);
        flows.remove_service_port_collisions(&service_ports);
        assert_eq!(flows.peer_counts.get(&source), Some(&2));
        assert!(!flows.flows.values().any(|flow| flow.reply.proto == 17 && flow.output.as_ref().is_some_and(|out| out.src_port == 40000)));
        assert!(flows.flows.values().any(|flow| flow.reply.proto == 17 && flow.output.as_ref().is_some_and(|out| out.src_port == 40001)));
        assert!(flows.flows.values().any(|flow| flow.reply.proto == 6 && flow.output.as_ref().is_some_and(|out| out.src_port == 40000)));
        assert!(!flows.reverse.keys().any(|reply| reply.proto == 17 && reply.dport == 40000));
    }
}
