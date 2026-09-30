//! Bounded bidirectional state. Pending reservations never grant reply access.
use std::{collections::{HashMap, HashSet}, net::Ipv4Addr, time::{Duration, Instant}};

use crate::{transport::ipv4::{PacketTuple, ValidatedPacket}, model::{Forward, Peer}};
#[cfg(test)]
use crate::transport::ipv4;

const CAPACITY: usize = 16_384;
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
    hub_ip: Ipv4Addr,
    service_ports: HashSet<(u8, u16)>,
    next_udp_snat: u16,
    next_tcp_snat: u16,
}

impl Default for Flows { fn default() -> Self { Self::new(Ipv4Addr::new(10, 77, 0, 1), &[]) } }

impl Flows {
    pub fn new(hub_ip: Ipv4Addr, forwards: &[Forward]) -> Self {
        let service_ports = forwards.iter().filter_map(|f| protocol(&f.protocol).map(|p| (p, f.target_port))).collect();
        Self { flows: HashMap::new(), reverse: HashMap::new(), pending: HashMap::new(), pending_reverse: HashMap::new(), hub_ip, service_ports, next_udp_snat: SNAT_START, next_tcp_snat: SNAT_START }
    }

    pub fn clear(&mut self) {
        self.flows.clear(); self.reverse.clear(); self.pending.clear(); self.pending_reverse.clear();
    }

    /// Reclaim expired records. Called at bounded sweep intervals and before capacity allocation.
    pub fn expire(&mut self, now: Instant) {
        self.flows.retain(|key, flow| now.saturating_duration_since(flow.last) < idle(key.protocol));
        self.reverse.retain(|_, key| self.flows.contains_key(key));
    }

    /// Prepare ordinary direct UDP delivery. `destination` identifies the peer
    /// that owns the packet's destination IP; reverse access is keyed to it.
    pub(crate) fn prepare_direct(&mut self, packet: &ValidatedPacket, source: &Peer, destination: &Peer, now: Instant) -> Option<(Vec<u8>, Reservation)> {
        let (sport, dport) = (packet.src_port()?, packet.dst_port()?);
        let proto = packet.protocol();
        if proto != 17 || packet.dst().to_string() != destination.ipv4 { return None; }
        let key = Tuple { peer: source.id.clone(), ip: packet.src(), port: sport, frontend_ip: packet.dst(), frontend_port: dport, protocol: proto };
        self.prepare_common(packet, key, destination, None, now)
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
            self.reserve_capacity(now)?;
            let snat = self.choose_snat(proto, backend, forward.target_port)?;
            let reply = Reverse { peer: backend.id.clone(), proto, src: backend_ip, sport: forward.target_port, dst: self.hub_ip, dport: snat };
            let flow = Flow { reply: reply.clone(), output: Some(PacketTuple { src: self.hub_ip, src_port: snat, dst: backend_ip, dst_port: forward.target_port }), backend: backend.id.clone(), backend_ip, last: now };
            self.pending_reverse.insert(reply, key.clone());
            self.pending.insert(key.clone(), flow.clone());
            flow
        };
        let is_new = !self.flows.contains_key(&key);
        let output = packet.clone().emit(flow.output);
        Some((output, Reservation { key, flow, is_new }))
    }

    fn prepare_common(&mut self, packet: &ValidatedPacket, key: Tuple, destination: &Peer, output: Option<PacketTuple>, now: Instant) -> Option<(Vec<u8>, Reservation)> {
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
            self.reserve_capacity(now)?;
            let reply = Reverse { peer: destination.id.clone(), proto: key.protocol, src: key.frontend_ip, sport: key.frontend_port, dst: key.ip, dport: key.port };
            let flow = Flow { reply: reply.clone(), output, backend: destination.id.clone(), backend_ip: destination_ip, last: now };
            self.pending_reverse.insert(reply, key.clone());
            self.pending.insert(key.clone(), flow.clone());
            flow
        };
        let is_new = !self.flows.contains_key(&key);
        Some((packet.clone().emit(flow.output), Reservation { key, flow, is_new }))
    }

    fn reserve_capacity(&mut self, now: Instant) -> Option<()> {
        if self.flows.len() + self.pending.len() >= CAPACITY { self.expire(now); }
        (self.flows.len() + self.pending.len() < CAPACITY).then_some(())
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
            if reservation.is_new && self.pending.get(&reservation.key).is_some_and(|f| f.reply == reservation.flow.reply) { self.remove_pending(&reservation.key); }
            return;
        }
        let mut flow = reservation.flow;
        flow.last = now;
        self.remove_pending(&reservation.key);
        self.reverse.insert(flow.reply.clone(), reservation.key.clone());
        self.flows.insert(reservation.key, flow);
    }

    fn remove_pending(&mut self, key: &Tuple) {
        self.pending.remove(key);
        self.pending_reverse.retain(|_, value| value != key);
    }

    fn remove_active(&mut self, key: &Tuple) {
        if let Some(flow) = self.flows.remove(key) { self.reverse.remove(&flow.reply); }
    }

    /// A live reverse hit is a normal delivery reservation and must be completed
    /// with its delivery result to refresh the flow's idle timer.
    pub(crate) fn lookup_reply(&mut self, packet: &ValidatedPacket, peer: &Peer, now: Instant) -> Option<(String, Vec<u8>, Reservation)> {
        let tuple = Reverse { peer: peer.id.clone(), proto: packet.protocol(), src: packet.src(), sport: packet.src_port()?, dst: packet.dst(), dport: packet.dst_port()? };
        let key = self.reverse.get(&tuple)?.clone();
        let flow = self.flows.get(&key)?.clone();
        if now.saturating_duration_since(flow.last) >= idle(key.protocol) { self.remove_active(&key); return None; }
        let output = PacketTuple { src: key.frontend_ip, src_port: key.frontend_port, dst: key.ip, dst_port: key.port };
        let packet = packet.clone().emit(Some(output));
        let target_peer = key.peer.clone();
        Some((target_peer, packet, Reservation { key, flow, is_new: false }))
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
        let checksum = ipv4::checksum(&bytes[..20]);
        bytes[10..12].copy_from_slice(&checksum.to_be_bytes());
        ipv4::validate_forwarded(&bytes).unwrap()
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
        assert!(flows.prepare_direct(&packet(6, "10.77.0.2".parse().unwrap(), 1234, "10.77.0.3".parse().unwrap(), 5678, 2), &src, &dst, now).is_none());
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
            let flow = Flow { reply: Reverse { peer: format!("b{n}"), proto: 17, src: Ipv4Addr::LOCALHOST, sport: 9, dst: Ipv4Addr::LOCALHOST, dport: n as u16 }, output: None, backend: "backend".into(), backend_ip: Ipv4Addr::LOCALHOST, last: now };
            full.pending_reverse.insert(flow.reply.clone(), key.clone()); full.pending.insert(key, flow);
        }
        assert!(full.reserve_capacity(now).is_none());

        let mut exhausted = Flows::default();
        for port in SNAT_START..=SNAT_END {
            exhausted.pending_reverse.insert(Reverse { peer: backend.id.clone(), proto: 17, src: backend.ipv4.parse().unwrap(), sport: 53, dst: exhausted.hub_ip, dport: port }, Tuple { peer: format!("used{port}"), ip: Ipv4Addr::LOCALHOST, port, frontend_ip: Ipv4Addr::LOCALHOST, frontend_port: 53, protocol: 17 });
        }
        assert!(exhausted.choose_snat(17, &backend, 53).is_none());
    }
}
