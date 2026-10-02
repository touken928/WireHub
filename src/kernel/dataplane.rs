//! Synchronous routing state. This module deliberately has no WireGuard, socket,
//! storage, or async dependencies; the runtime only adapts its egress boundary.
use std::{
    collections::{HashMap, VecDeque},
    net::Ipv4Addr,
    time::{Duration, Instant},
};

use crate::kernel::{
    flows::{Flows, Reservation},
    ipv4::{self, ValidatedPacket},
    policy,
    protocol::{FlowAssociation, RewritePlan},
    snapshot::{ForwardConfig, PeerConfig, PeerConfigView, PeerKey, TransportProtocol},
};

const PENDING_TTL: Duration = Duration::from_secs(3);
const PENDING_LIMIT: usize = 256;
const PENDING_BYTES: usize = 1024 * 1024;

struct PendingPacket {
    ingress: IngressPacket,
    stamp: RouteStamp,
    deadline: Instant,
}
#[derive(Default)]
struct PendingQueue {
    entries: VecDeque<PendingPacket>,
    bytes: usize,
}

#[derive(Clone, Default)]
pub(crate) struct RoutingConfig {
    pub peers: HashMap<String, PeerConfig>,
    pub forwards: Vec<ForwardConfig>,
    pub hub_ip: Option<Ipv4Addr>,
    pub by_ip: HashMap<Ipv4Addr, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct AuthenticatedPeer {
    id: String,
    key: PeerKey,
    ip: Ipv4Addr,
}
#[derive(Clone)]
pub(crate) struct IngressPacket {
    source_id: String,
    packet: ValidatedPacket,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ForwardIdentity {
    Forward {
        id: String,
        protocol: TransportProtocol,
        target_port: u16,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RouteOrigin {
    Direct,
    Forward(ForwardIdentity),
    ReplyOnly,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RouteStamp {
    source_id: String,
    source_key: PeerKey,
    source_ip: Ipv4Addr,
    target_id: String,
    target_key: PeerKey,
    target_ip: Ipv4Addr,
    origin: RouteOrigin,
}

#[must_use = "a prepared delivery must be finished exactly once"]
pub(crate) struct PreparedDelivery {
    ingress: IngressPacket,
    stamp: RouteStamp,
    bytes: Vec<u8>,
    reservation: Option<Reservation>,
    original_deadline: Option<Instant>,
}
impl PreparedDelivery {
    pub(crate) fn target_id(&self) -> &str {
        &self.stamp.target_id
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EgressOutcome {
    Delivered,
    Failed,
    NotReady,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeliveryAccounting {
    pub source_id: String,
    pub target_id: String,
    pub bytes: u64,
}

pub(crate) struct DataPlane {
    pub config: RoutingConfig,
    flows: Flows,
    pending: PendingQueue,
}
impl Default for DataPlane {
    fn default() -> Self {
        Self {
            config: RoutingConfig::default(),
            flows: Flows::default(),
            pending: PendingQueue::default(),
        }
    }
}
impl DataPlane {
    pub(crate) fn authenticated_peer(&self, id: &str) -> Option<AuthenticatedPeer> {
        let peer = self.config.peers.get(id)?;
        Some(AuthenticatedPeer {
            id: id.to_owned(),
            key: peer.key_identity(),
            ip: peer.ip,
        })
    }
    pub(crate) fn clear(&mut self) {
        self.config = RoutingConfig::default();
        self.flows.clear();
        self.pending = PendingQueue::default();
    }
    pub(crate) fn expire(&mut self, now: Instant) {
        self.flows.expire(now);
    }
    pub(crate) fn reconcile(&mut self, old: &RoutingConfig, new: RoutingConfig) {
        let peers = new.peers.clone();
        self.flows
            .reconcile(old.hub_ip, new.hub_ip, &old.forwards, &new.forwards, |id| {
                peers.get(id).map(|p| policy::PeerPolicy {
                    peer: p,
                    group: p.group.as_ref(),
                })
            });
        self.config = new;
        let now = Instant::now();
        let mut kept = VecDeque::new();
        let mut bytes = 0usize;
        while let Some(item) = self.pending.entries.pop_front() {
            if item.deadline > now && self.provenance_valid(&item.ingress, &item.stamp, now) {
                bytes += item.ingress.packet.bytes().len();
                kept.push_back(item);
            }
        }
        self.pending.entries = kept;
        self.pending.bytes = bytes;
    }
    pub(crate) fn ingress(
        &self,
        authenticated: &AuthenticatedPeer,
        bytes: &[u8],
    ) -> Option<IngressPacket> {
        let source = self.config.peers.get(&authenticated.id)?;
        if source.key_identity() != authenticated.key || source.ip != authenticated.ip {
            return None;
        }
        let packet = ipv4::validate(bytes, source, source.group.as_ref())?;
        Some(IngressPacket {
            source_id: authenticated.id.clone(),
            packet,
        })
    }
    pub(crate) fn prepare(
        &mut self,
        ingress: IngressPacket,
        now: Instant,
    ) -> Option<PreparedDelivery> {
        self.prepare_inner(ingress, now, None, None)
    }
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.entries.len()
    }
    pub(crate) fn prepare_retry(
        &mut self,
        remaining: &mut usize,
        now: Instant,
    ) -> Option<PreparedDelivery> {
        while *remaining > 0 {
            let Some(item) = self.pending.entries.pop_front() else {
                break;
            };
            *remaining -= 1;
            self.pending.bytes = self
                .pending
                .bytes
                .saturating_sub(item.ingress.packet.bytes().len());
            if item.deadline <= now {
                continue;
            }
            if !self.provenance_valid(&item.ingress, &item.stamp, now) {
                continue;
            }
            let expected = item.stamp.clone();
            if let Some(prepared) = self.prepare_inner(
                item.ingress.clone(),
                now,
                Some(item.deadline),
                Some(expected.clone()),
            ) {
                if prepared.stamp == expected {
                    return Some(prepared);
                }
                let _ = self.finish(prepared, EgressOutcome::Failed, now);
            }
        }
        None
    }
    fn prepare_inner(
        &mut self,
        ingress: IngressPacket,
        now: Instant,
        deadline: Option<Instant>,
        expected: Option<RouteStamp>,
    ) -> Option<PreparedDelivery> {
        let source = self.config.peers.get(&ingress.source_id)?.clone();
        let packet = &ingress.packet;
        let mut plan = None;
        let mut origin = RouteOrigin::Direct;
        if expected
            .as_ref()
            .is_some_and(|stamp| stamp.origin == RouteOrigin::ReplyOnly)
        {
            let (target_id, rewrite, reservation) =
                self.flows.lookup_reply(packet, &source, now)?;
            let (bytes, reservation) = rewrite_reserved(
                &mut self.flows,
                packet,
                rewrite,
                reservation,
                now,
                |p, r| p.rewrite(r),
            )?;
            plan = Some((target_id, bytes, reservation));
            origin = RouteOrigin::ReplyOnly;
        } else if let Some((target_id, rewrite, reservation)) =
            self.flows.lookup_reply(packet, &source, now)
        {
            let (bytes, reservation) = rewrite_reserved(
                &mut self.flows,
                packet,
                rewrite,
                reservation,
                now,
                |p, r| p.rewrite(r),
            )?;
            plan = Some((target_id, bytes, reservation));
            origin = RouteOrigin::ReplyOnly;
        } else {
            if matches!(packet.association(), FlowAssociation::Related { .. }) {
                return None;
            }
            if packet.dst() == self.config.hub_ip.unwrap_or(Ipv4Addr::UNSPECIFIED) {
                for forward in &self.config.forwards {
                    let Some(target) = self.config.peers.get(&forward.target_peer_id) else {
                        continue;
                    };
                    if !policy::forward_allowed(
                        forward,
                        &source.group_id,
                        source.group.as_ref(),
                        &target.group_id,
                    ) {
                        continue;
                    }
                    let Some((rewrite, reservation)) = self
                        .flows
                        .prepare_forward_packet(packet, &source, forward, target, now)
                    else {
                        continue;
                    };
                    let (bytes, reservation) = rewrite_reserved(
                        &mut self.flows,
                        packet,
                        rewrite,
                        Some(reservation),
                        now,
                        |p, r| p.rewrite(r),
                    )?;
                    origin = RouteOrigin::Forward(ForwardIdentity::Forward {
                        id: forward.id.clone(),
                        protocol: forward.protocol,
                        target_port: forward.target_port,
                    });
                    plan = Some((target.id.clone(), bytes, reservation));
                    break;
                }
            }
            if plan.is_none() {
                if let Some(target) = self
                    .config
                    .by_ip
                    .get(&packet.dst())
                    .and_then(|id| self.config.peers.get(id))
                {
                    if policy::route_allowed(source.group.as_ref(), target.group.as_ref()) {
                        if matches!(packet.association(), FlowAssociation::Stateless) {
                            plan = Some((target.id.clone(), packet.bytes().to_vec(), None));
                        } else if let Some((rewrite, reservation)) =
                            self.flows.prepare_direct(packet, &source, target, now)
                        {
                            let (bytes, reservation) = rewrite_reserved(
                                &mut self.flows,
                                packet,
                                rewrite,
                                Some(reservation),
                                now,
                                |p, r| p.rewrite(r),
                            )?;
                            plan = Some((target.id.clone(), bytes, reservation));
                        }
                    }
                }
            }
        }
        let (target_id, bytes, reservation) = plan?;
        let target = self.config.peers.get(&target_id)?;
        let stamp = RouteStamp {
            source_id: source.id.clone(),
            source_key: source.key_identity(),
            source_ip: source.ip,
            target_id,
            target_key: target.key_identity(),
            target_ip: target.ip,
            origin,
        };
        Some(PreparedDelivery {
            ingress,
            stamp,
            bytes,
            reservation,
            original_deadline: deadline,
        })
    }
    pub(crate) fn finish(
        &mut self,
        mut prepared: PreparedDelivery,
        outcome: EgressOutcome,
        now: Instant,
    ) -> Option<DeliveryAccounting> {
        if let Some(reservation) = prepared.reservation.take() {
            self.flows
                .complete(reservation, outcome == EgressOutcome::Delivered, now);
        }
        match outcome {
            EgressOutcome::Delivered => Some(DeliveryAccounting {
                source_id: prepared.stamp.source_id,
                target_id: prepared.stamp.target_id,
                bytes: prepared.bytes.len() as u64,
            }),
            EgressOutcome::Failed => None,
            EgressOutcome::NotReady => {
                let deadline = prepared.original_deadline.unwrap_or(now + PENDING_TTL);
                if deadline > now && self.provenance_valid(&prepared.ingress, &prepared.stamp, now)
                {
                    let size = prepared.ingress.packet.bytes().len();
                    if self.pending.entries.len() < PENDING_LIMIT
                        && self.pending.bytes.saturating_add(size) <= PENDING_BYTES
                    {
                        self.pending.bytes += size;
                        self.pending.entries.push_back(PendingPacket {
                            ingress: prepared.ingress,
                            stamp: prepared.stamp,
                            deadline,
                        });
                    }
                }
                None
            }
        }
    }
    fn provenance_valid(&self, ingress: &IngressPacket, stamp: &RouteStamp, now: Instant) -> bool {
        let (Some(source), Some(target)) = (
            self.config.peers.get(&stamp.source_id),
            self.config.peers.get(&stamp.target_id),
        ) else {
            return false;
        };
        if ingress.source_id != stamp.source_id
            || ingress.packet.src() != stamp.source_ip
            || source.key_identity() != stamp.source_key
            || source.ip != stamp.source_ip
            || target.key_identity() != stamp.target_key
            || target.ip != stamp.target_ip
        {
            return false;
        }
        match &stamp.origin {
            RouteOrigin::ReplyOnly => self.flows.has_reply_mapping(&ingress.packet, source, now),
            RouteOrigin::Direct => {
                policy::route_allowed(source.group.as_ref(), target.group.as_ref())
                    && (ingress.packet.dst() == target.ip)
            }
            RouteOrigin::Forward(ForwardIdentity::Forward {
                id,
                protocol,
                target_port,
            }) => self.config.forwards.iter().any(|f| {
                &f.id == id
                    && f.protocol == *protocol
                    && f.target_port == *target_port
                    && f.target_peer_id == target.id
                    && ingress.packet.protocol() == protocol.number()
                    && ingress.packet.dst() == self.config.hub_ip.unwrap_or(Ipv4Addr::UNSPECIFIED)
                    && ingress.packet.dst_port() == Some(*target_port)
                    && policy::forward_allowed(
                        f,
                        &source.group_id,
                        source.group.as_ref(),
                        &target.group_id,
                    )
            }),
        }
    }
    #[cfg(test)]
    pub(crate) fn flow_counts(&self) -> (usize, usize) {
        self.flows.test_state_counts()
    }
    #[cfg(test)]
    pub(crate) fn pending_counts(&self) -> (usize, usize) {
        (self.pending.entries.len(), self.pending.bytes)
    }
    #[cfg(test)]
    pub(crate) fn pending_queue_bytes(&self) -> usize {
        self.pending.bytes
    }
    #[cfg(test)]
    pub(crate) fn test_pending_observation(
        &self,
    ) -> Vec<(
        String,
        String,
        Option<String>,
        Ipv4Addr,
        Ipv4Addr,
        Ipv4Addr,
        u16,
        u8,
    )> {
        self.pending
            .entries
            .iter()
            .map(|item| {
                let fid = match &item.stamp.origin {
                    RouteOrigin::Forward(ForwardIdentity::Forward { id, .. }) => Some(id.clone()),
                    _ => None,
                };
                (
                    item.stamp.source_id.clone(),
                    item.stamp.target_id.clone(),
                    fid,
                    item.ingress.packet.src(),
                    item.ingress.packet.dst(),
                    item.stamp.target_ip,
                    item.ingress.packet.src_port().unwrap_or_default(),
                    item.ingress.packet.protocol(),
                )
            })
            .collect()
    }
}

fn rewrite_reserved(
    flows: &mut Flows,
    packet: &ValidatedPacket,
    rewrite: RewritePlan,
    reservation: Option<Reservation>,
    now: Instant,
    apply: impl FnOnce(ValidatedPacket, RewritePlan) -> Option<Vec<u8>>,
) -> Option<(Vec<u8>, Option<Reservation>)> {
    match apply(packet.clone(), rewrite) {
        Some(bytes) => Some((bytes, reservation)),
        None => {
            if let Some(reservation) = reservation {
                flows.complete(reservation, false, now);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests;
