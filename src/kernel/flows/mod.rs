//! Bounded bidirectional state. Pending reservations never grant reply access.
use std::{collections::{HashMap, HashSet}, net::Ipv4Addr, time::Instant};

use crate::model::Forward;
use crate::kernel::snapshot::{PeerConfigView, PeerKey};
use super::{policy::{self, PeerPolicy}, ipv4::ValidatedPacket, protocol::{PacketTuple, FlowAssociation, FlowEvent, FlowState, RewritePlan}};
#[cfg(test)]
use crate::kernel::{ipv4, protocol::{TcpState, TCP_ESTABLISHED_IDLE, TCP_HANDSHAKE_IDLE, TCP_CLOSED_GRACE, UDP_IDLE}};
#[cfg(test)]
use std::time::Duration;

const CAPACITY: usize = 16_384;
/// Maximum active and pending flows initiated by one peer, across TCP and UDP.
const PEER_CAPACITY: usize = 256;
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
    initiator_key: PeerKey,
    backend_key: PeerKey,
    forward_id: Option<String>,
    last: Instant,
    generation: u64,
    state: FlowState,
}

/// A delivery must call `complete` exactly once. Failed delivery releases a new
/// reservation; only successful delivery installs or refreshes active state.
pub(crate) struct Reservation { key: Tuple, flow: Flow, is_new: bool, from_initiator: bool, event: FlowEvent }

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
    earliest_expiry: Option<Instant>,
    next_generation: Option<u64>,
    #[cfg(test)]
    expiry_scans: usize,
}

impl Default for Flows { fn default() -> Self { Self::new(Ipv4Addr::new(10, 77, 0, 1), &[]) } }

impl Flows {
    #[cfg(test)]
    pub(crate) fn test_state_counts(&self) -> (usize, usize) { (self.flows.len(), self.pending.len()) }

    pub fn new(hub_ip: Ipv4Addr, forwards: &[Forward]) -> Self {
        let service_ports = forwards.iter().filter_map(|f| protocol(&f.protocol).map(|p| (p, f.target_port))).collect();
        Self { flows: HashMap::new(), reverse: HashMap::new(), pending: HashMap::new(), pending_reverse: HashMap::new(), peer_counts: HashMap::new(), hub_ip, service_ports, next_udp_snat: SNAT_START, next_tcp_snat: SNAT_START, earliest_expiry: None, next_generation: Some(1), #[cfg(test)] expiry_scans: 0 }
    }

    pub fn clear(&mut self) {
        self.flows.clear(); self.reverse.clear(); self.pending.clear(); self.pending_reverse.clear(); self.peer_counts.clear();
        self.earliest_expiry = None;
    }

    pub(crate) fn reconcile<'a>(&mut self, old_hub: Option<Ipv4Addr>, new_hub: Option<Ipv4Addr>, old_forwards: &[Forward], new_forwards: &[Forward], peer_policy: impl Fn(&str) -> Option<PeerPolicy<'a>>) {
        let same_hub = old_hub == new_hub;
        self.flows.retain(|key, flow| {
            if !same_hub { return false; }
            let Some(source) = peer_policy(&key.peer) else { return false };
            let Some(target) = peer_policy(&flow.backend) else { return false };
            if source.peer.key_identity() != flow.initiator_key || target.peer.key_identity() != flow.backend_key || source.peer.ip() != key.ip || target.peer.ip() != flow.backend_ip { return false; }
            if flow.forward_id.is_some() {
                let Some(id) = flow.forward_id.as_deref() else { return false };
                let Some(before) = old_forwards.iter().find(|f| f.id == id) else { return false };
                let Some(after) = new_forwards.iter().find(|f| f.id == id) else { return false };
                if before.protocol != after.protocol || before.target_peer_id != after.target_peer_id || before.target_port != after.target_port || !after.allowed_group_ids.iter().any(|id|id==source.peer.group_id()) || !policy::forward_allowed(after, source.peer.group_id(), source.group, target.peer.group_id()) { return false; }
            } else if !policy::route_allowed(source.group, target.group) { return false; }
            true
        });
        let service_ports: HashSet<_> = new_forwards.iter().filter_map(|f| protocol(&f.protocol).map(|p| (p, f.target_port))).collect();
        self.remove_service_port_collisions(&service_ports);
        self.reverse.retain(|_,key| self.flows.contains_key(key));
        self.pending.clear(); self.pending_reverse.clear();
        self.rebuild_peer_counts();
        self.recompute_earliest_expiry();
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
        #[cfg(test)] { self.expiry_scans += 1; }
        self.flows.retain(|_, flow| flow.deadline() > now);
        self.reverse.retain(|_, key| self.flows.contains_key(key));
        self.pending.retain(|_, flow| flow.deadline() > now);
        self.pending_reverse.retain(|_, key| self.pending.contains_key(key));
        self.rebuild_peer_counts();
        self.recompute_earliest_expiry();
    }

    /// Direct and translated delivery share allocation, quotas and commit semantics.
    pub(crate) fn prepare_direct(&mut self, packet: &ValidatedPacket, source: &impl PeerConfigView, destination: &impl PeerConfigView, now: Instant) -> Option<(Vec<u8>, Reservation)> {
        if packet.dst() != destination.ip() { return None; }
        self.prepare(packet, source, destination, None, now)
    }

    pub(crate) fn prepare_forward_packet(&mut self, packet: &ValidatedPacket, source: &impl PeerConfigView, forward: &Forward, backend: &impl PeerConfigView, now: Instant) -> Option<(Vec<u8>, Reservation)> {
        if packet.protocol() != protocol(&forward.protocol)? || packet.dst() != self.hub_ip || packet.dst_port()? != forward.target_port { return None; }
        self.prepare(packet, source, backend, Some(forward), now)
    }

    fn prepare(&mut self, packet: &ValidatedPacket, source: &impl PeerConfigView, destination: &impl PeerConfigView, forward: Option<&Forward>, now: Instant) -> Option<(Vec<u8>, Reservation)> {
        let FlowAssociation::Connection { protocol, tuple, event } = packet.association() else { return None };
        let key = Tuple { peer: source.id().to_owned(), ip: tuple.src, port: tuple.src_port, frontend_ip: tuple.dst, frontend_port: tuple.dst_port, protocol };
        // An expired tuple is a new flow and must satisfy its protocol's initiation rule.
        if self.flows.get(&key).is_some_and(|flow| flow.deadline() <= now) { self.remove_active(&key); }
        let destination_ip = destination.ip();
        let flow = if let Some(flow) = self.flows.get(&key).cloned() {
            if flow.backend != destination.id() || flow.backend_ip != destination_ip || flow.forward_id.as_deref() != forward.map(|f| f.id.as_str()) { return None; }
            if event.starts_new_tcp() && flow.state.is_closing() { return None; }
            flow
        } else {
            // A pending token has a single owner; failed delivery must not release
            // another packet's reservation for the same tuple.
            if self.pending.contains_key(&key) { return None; }
            let state = event.initial_state()?;
            self.reserve_capacity(&key.peer, now)?;
            let output = if let Some(forward) = forward {
                Some(PacketTuple { src: self.hub_ip, src_port: self.choose_snat(protocol, destination, forward.target_port)?, dst: destination_ip, dst_port: forward.target_port })
            } else { None };
            let wire = output.unwrap_or(tuple);
            let reply = Reverse { peer: destination.id().to_owned(), proto: protocol, src: wire.dst, sport: wire.dst_port, dst: wire.src, dport: wire.src_port };
            let generation = self.allocate_generation()?;
            let flow = Flow { reply: reply.clone(), output, backend: destination.id().to_owned(), backend_ip: destination_ip, initiator_key: source.key_identity(), backend_key: destination.key_identity(), forward_id: forward.map(|f| f.id.clone()), last: now, generation, state };
            self.pending_reverse.insert(reply, key.clone());
            self.pending.insert(key.clone(), flow.clone());
            self.note_expiry(flow.deadline());
            self.add_peer_count(&key.peer);
            flow
        };
        let is_new = !self.flows.contains_key(&key);
        let bytes = packet.clone().rewrite(flow.output.map_or(RewritePlan::Keep, RewritePlan::Transport))?;
        Some((bytes, Reservation { key, flow, is_new, from_initiator: true, event }))
    }

    fn reserve_capacity(&mut self, peer: &str, now: Instant) -> Option<()> {
        // Sweep at the relevant boundary too: a peer can be full long before
        // the global table is, and expired records must not strand its quota.
        if (self.flows.len() + self.pending.len() >= CAPACITY
            || self.peer_counts.get(peer).copied().unwrap_or(0) >= PEER_CAPACITY
        ) && self.earliest_expiry.is_some_and(|deadline| deadline <= now) {
            self.expire(now);
        }
        (self.flows.len() + self.pending.len() < CAPACITY
            && self.peer_counts.get(peer).copied().unwrap_or(0) < PEER_CAPACITY).then_some(())
    }

    fn choose_snat(&mut self, proto: u8, backend: &impl PeerConfigView, target_port: u16) -> Option<u16> {
        let backend_ip = backend.ip();
        let slots = (SNAT_END - SNAT_START + 1) as usize;
        let counter = if proto == 6 { &mut self.next_tcp_snat } else { &mut self.next_udp_snat };
        for _ in 0..slots {
            let candidate = *counter;
            *counter = if candidate == SNAT_END { SNAT_START } else { candidate + 1 };
            let idx = Reverse { peer: backend.id().to_owned(), proto, src: backend_ip, sport: target_port, dst: self.hub_ip, dport: candidate };
            if !self.service_ports.contains(&(proto, candidate)) && !self.reverse.contains_key(&idx) && !self.pending_reverse.contains_key(&idx) { return Some(candidate); }
        }
        None
    }

    /// Commit/refresh only after successful business-data delivery; failure only releases pending state.
    pub fn complete(&mut self, reservation: Reservation, delivered: bool, now: Instant) {
        if !delivered {
            if reservation.is_new && self.pending.get(&reservation.key).is_some_and(|f| f.generation == reservation.flow.generation) { self.remove_pending(&reservation.key); }
            return;
        }
        // Expiry, clear, or reconciliation may have invalidated a queued
        // reservation while delivery was outstanding. Do not resurrect it.
        let valid = if reservation.is_new {
            self.pending.get(&reservation.key).is_some_and(|f| f.generation == reservation.flow.generation)
        } else {
            self.flows.get(&reservation.key).is_some_and(|f| f.generation == reservation.flow.generation)
        };
        if !valid {
            return;
        }
        let key = reservation.key.clone();
        let mut flow = reservation.flow;
        if let Some(current) = self.flows.get(&reservation.key) {
            // Use the most recently committed protocol state when multiple
            // deliveries were prepared before either completion was processed.
            flow.state = current.state;
            flow.last = flow.last.max(current.last);
        }
        if flow.state.on_delivered(reservation.event, reservation.from_initiator, now) { flow.last = flow.last.max(now); }
        if reservation.is_new { self.detach_pending(&reservation.key); }
        self.reverse.insert(flow.reply.clone(), key.clone());
        let deadline = flow.deadline();
        self.flows.insert(key, flow);
        self.note_expiry(deadline);
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
    pub(crate) fn cancel_pending_packet(&mut self, packet: &ValidatedPacket, source: &impl PeerConfigView) {
        let (Some(port),Some(frontend_port))=(packet.src_port(),packet.dst_port()) else { return };
        let key=Tuple{peer:source.id().to_owned(),ip:packet.src(),port,frontend_ip:packet.dst(),frontend_port,protocol:packet.protocol()};
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

    fn note_expiry(&mut self, deadline: Instant) {
        self.earliest_expiry = Some(self.earliest_expiry.map_or(deadline, |current| current.min(deadline)));
    }

    fn allocate_generation(&mut self) -> Option<u64> {
        let generation = self.next_generation?;
        self.next_generation = generation.checked_add(1);
        Some(generation)
    }

    fn recompute_earliest_expiry(&mut self) {
        self.earliest_expiry = self.flows.iter().chain(self.pending.iter())
            .map(|(_, flow)| flow.deadline()).min();
    }

    /// Resolve both transport replies and related ICMP errors through the same
    /// committed reverse index. Related errors never create or refresh state.
    pub(crate) fn lookup_reply(&mut self, packet: &ValidatedPacket, peer: &impl PeerConfigView, now: Instant) -> Option<(String, Vec<u8>, Reservation)> {
        let key = self.reply_key(packet, peer)?.clone();
        let flow = self.flows.get(&key)?.clone();
        if flow.deadline() <= now { self.remove_active(&key); return None; }
        let plan = match packet.association() {
            FlowAssociation::Connection { .. } => RewritePlan::Transport(PacketTuple { src: key.frontend_ip, src_port: key.frontend_port, dst: key.ip, dst_port: key.port }),
            FlowAssociation::Related { .. } => RewritePlan::Related { original: PacketTuple { src: key.ip, src_port: key.port, dst: key.frontend_ip, dst_port: key.frontend_port }, sender: key.frontend_ip, recipient: key.ip },
            FlowAssociation::Stateless => return None,
        };
        let bytes = packet.clone().rewrite(plan)?;
        Some((key.peer.clone(), bytes, Reservation { key, flow, is_new: false, from_initiator: false, event: packet.association().event() }))
    }

    fn reply_key(&self, packet: &ValidatedPacket, peer: &impl PeerConfigView) -> Option<&Tuple> {
        let (reverse, related) = match packet.association() {
            FlowAssociation::Connection { protocol, tuple, event } => {
                if event.starts_new_tcp() { return None; }
                (Reverse { peer: peer.id().to_owned(), proto: protocol, src: tuple.src, sport: tuple.src_port, dst: tuple.dst, dport: tuple.dst_port }, false)
            }
            FlowAssociation::Related { protocol, tuple } => {
                if packet.dst() != tuple.src { return None; }
                (Reverse { peer: peer.id().to_owned(), proto: protocol, src: tuple.dst, sport: tuple.dst_port, dst: tuple.src, dport: tuple.src_port }, true)
            }
            FlowAssociation::Stateless => return None,
        };
        let key = self.reverse.get(&reverse)?;
        let flow = self.flows.get(key)?;
        if flow.backend_key != peer.key_identity() || (related && packet.src() != peer.ip()) { return None; }
        Some(key)
    }

    pub(crate) fn has_reply_mapping(&self, packet: &ValidatedPacket, peer: &impl PeerConfigView, now: Instant) -> bool {
        self.reply_key(packet, peer).and_then(|key| self.flows.get(key).map(|flow| flow.deadline() > now)).unwrap_or(false)
    }
}

impl Flow { fn deadline(&self) -> Instant { self.state.deadline(self.last) } }
fn protocol(value: &str) -> Option<u8> { match value { "tcp" => Some(6), "udp" => Some(17), _ => None } }

#[cfg(test)]
mod tests;
