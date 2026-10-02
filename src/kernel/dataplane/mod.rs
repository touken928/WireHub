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
mod pending;
use pending::{PendingPacket, PendingQueue, PENDING_BYTES, PENDING_LIMIT};

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
mod tests {
    use super::*;
    use crate::{kernel::ipv4, model::Group};

    fn peer(id: &str, ip: [u8; 4], key: u8) -> PeerConfig {
        PeerConfig {
            id: id.into(),
            key: [key; 32],
            ip: Ipv4Addr::from(ip),
            group_id: "g".into(),
            group: Some(Group {
                id: "g".into(),
                name: "g".into(),
                allowed_groups: vec!["g".into()],
            }),
        }
    }
    fn plane() -> DataPlane {
        let a = peer("a", [10, 77, 0, 2], 1);
        let b = peer("b", [10, 77, 0, 3], 2);
        let mut config = RoutingConfig::default();
        config.by_ip.insert(b.ip, "b".into());
        config.by_ip.insert(a.ip, "a".into());
        config.peers.insert("a".into(), a);
        config.peers.insert("b".into(), b);
        let mut dp = DataPlane::default();
        dp.config = config;
        dp
    }
    fn request() -> Vec<u8> {
        ipv4::test_packet([10, 77, 0, 2], [10, 77, 0, 3], false, false)
    }
    fn ingress(dp: &DataPlane, raw: &[u8]) -> IngressPacket {
        dp.ingress(&dp.authenticated_peer("a").unwrap(), raw)
            .unwrap()
    }

    #[test]
    fn pending_is_bounded_expires_and_ttl_is_decremented_only_on_ingress() {
        let mut dp = plane();
        let raw = request();
        let now = Instant::now();
        for _ in 0..=PENDING_LIMIT {
            let prepared = dp.prepare(ingress(&dp, &raw), now).unwrap();
            assert_eq!(prepared.bytes()[8], 63);
            dp.finish(prepared, EgressOutcome::NotReady, now);
        }
        assert_eq!(
            dp.pending_counts(),
            (PENDING_LIMIT, PENDING_LIMIT * raw.len())
        );
        let mut remaining = dp.pending_len();
        let retry = dp
            .prepare_retry(&mut remaining, now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(retry.bytes()[8], 63, "retry must not decrement TTL again");
        dp.finish(retry, EgressOutcome::NotReady, now + Duration::from_secs(1));
        let mut remaining = usize::MAX;
        assert!(dp
            .prepare_retry(&mut remaining, now + Duration::from_secs(4))
            .is_none());
        assert_eq!(dp.pending_counts(), (0, 0));
    }

    #[test]
    fn stale_authenticated_identity_and_pending_route_provenance_are_rejected() {
        let mut dp = plane();
        let raw = request();
        let auth = dp.authenticated_peer("a").unwrap();
        let prepared = dp
            .prepare(dp.ingress(&auth, &raw).unwrap(), Instant::now())
            .unwrap();
        dp.finish(prepared, EgressOutcome::NotReady, Instant::now());
        let old = dp.config.clone();
        let mut new = old.clone();
        new.peers.insert("b".into(), peer("b", [10, 77, 0, 4], 2));
        new.by_ip.remove(&Ipv4Addr::new(10, 77, 0, 3));
        new.by_ip.insert(Ipv4Addr::new(10, 77, 0, 4), "b".into());
        dp.reconcile(&old, new);
        assert_eq!(dp.pending_counts(), (0, 0));
        assert!(
            dp.ingress(&auth, &raw).is_some(),
            "unchanged authenticated source remains valid"
        );
        let stale = dp.authenticated_peer("b").unwrap();
        let old_config = dp.config.clone();
        let mut changed = old_config.clone();
        changed
            .peers
            .insert("b".into(), peer("b", [10, 77, 0, 5], 9));
        dp.reconcile(&old_config, changed);
        assert!(dp.ingress(&stale, &raw).is_none());
    }

    #[test]
    fn reply_only_retry_does_not_fall_back_after_mapping_expires() {
        for retry_at in [
            Duration::from_secs(60),
            Duration::from_secs(60) + Duration::from_millis(1),
        ] {
            let mut dp = plane();
            let now = Instant::now();
            let raw = request();
            let request = dp.prepare(ingress(&dp, &raw), now).unwrap();
            assert!(dp.finish(request, EgressOutcome::Delivered, now).is_some());
            let mut reply = ipv4::test_packet([10, 77, 0, 3], [10, 77, 0, 2], false, false);
            reply[20..22].copy_from_slice(&5678u16.to_be_bytes());
            reply[22..24].copy_from_slice(&1234u16.to_be_bytes());
            reply[10..12].fill(0);
            let sum = crate::kernel::checksum::checksum(&reply[..20]);
            reply[10..12].copy_from_slice(&sum.to_be_bytes());
            assert!(
                policy::route_allowed(
                    dp.config.peers.get("b").unwrap().group.as_ref(),
                    dp.config.peers.get("a").unwrap().group.as_ref()
                ),
                "reverse direct route is otherwise ACL-permitted"
            );
            let auth = dp.authenticated_peer("b").unwrap();
            let packet = dp.ingress(&auth, &reply).unwrap();
            let delivery = dp.prepare(packet, now).unwrap();
            assert_eq!(delivery.stamp.origin, RouteOrigin::ReplyOnly);
            dp.finish(
                delivery,
                EgressOutcome::NotReady,
                now + Duration::from_secs(59),
            );
            assert_eq!(dp.pending_counts(), (1, reply.len()));
            assert_eq!(
                dp.pending_counts(),
                (1, reply.len()),
                "queue deadline is later than flow expiry before retry"
            );
            let mut remaining = usize::MAX;
            assert!(
                dp.prepare_retry(&mut remaining, now + retry_at).is_none(),
                "expired reply mapping must not fall back to direct route"
            );
            // Flow expiration is reclaimed by the normal bounded sweep, not by
            // an unrelated reply lookup/retry.
            dp.expire(now + retry_at);
            assert_eq!(dp.flow_counts(), (0, 0), "retry creates no new direct flow");
            assert_eq!(dp.pending_counts(), (0, 0));
        }
    }

    #[test]
    fn rewrite_failure_releases_new_reservation_and_preserves_active_flow() {
        let mut dp = plane();
        let now = Instant::now();
        let raw = request();
        let ingress = ingress(&dp, &raw);
        let source = dp.config.peers.get("a").unwrap().clone();
        let target = dp.config.peers.get("b").unwrap().clone();
        let (rewrite, reservation) = dp
            .flows
            .prepare_direct(&ingress.packet, &source, &target, now)
            .unwrap();
        assert!(rewrite_reserved(
            &mut dp.flows,
            &ingress.packet,
            rewrite,
            Some(reservation),
            now,
            |_, _| None
        )
        .is_none());
        assert_eq!(dp.flow_counts(), (0, 0));
        assert_eq!(dp.flows.test_index_counts(), (0, 0, 0));
        let (_, retry_reservation) = dp
            .flows
            .prepare_direct(&ingress.packet, &source, &target, now)
            .expect("same tuple can be prepared again");
        dp.flows.complete(retry_reservation, false, now);

        let (_rewrite, reservation) = dp
            .flows
            .prepare_direct(&ingress.packet, &source, &target, now)
            .unwrap();
        dp.flows.complete(reservation, true, now);
        let before = dp.flows.test_active_observation().unwrap().0;
        let (rewrite, reservation) = dp
            .flows
            .prepare_direct(
                &ingress.packet,
                &source,
                &target,
                now + Duration::from_secs(1),
            )
            .unwrap();
        assert!(rewrite_reserved(
            &mut dp.flows,
            &ingress.packet,
            rewrite,
            Some(reservation),
            now + Duration::from_secs(1),
            |_, _| None
        )
        .is_none());
        assert_eq!(dp.flow_counts(), (1, 0));
        assert_eq!(
            dp.flows.test_active_observation().unwrap().0,
            before,
            "failed rewrite does not refresh active flow"
        );
        assert_eq!(dp.flows.test_index_counts().0, 1);
    }

    #[test]
    fn pending_deadline_is_not_extended_by_retries() {
        let mut dp = plane();
        let raw = request();
        let now = Instant::now();
        let prepared = dp.prepare(ingress(&dp, &raw), now).unwrap();
        dp.finish(prepared, EgressOutcome::NotReady, now);
        for secs in [1, 2] {
            let mut remaining = dp.pending_len();
            let retry = dp
                .prepare_retry(&mut remaining, now + Duration::from_secs(secs))
                .unwrap();
            dp.finish(
                retry,
                EgressOutcome::NotReady,
                now + Duration::from_secs(secs),
            );
        }
        assert_eq!(dp.pending_counts(), (1, raw.len()));
        let mut remaining = dp.pending_len();
        let retry = dp
            .prepare_retry(&mut remaining, now + Duration::from_millis(2999))
            .expect("delivery remains live until original deadline");
        dp.finish(
            retry,
            EgressOutcome::NotReady,
            now + Duration::from_millis(2999),
        );
        assert_eq!(dp.pending_counts(), (1, raw.len()));
        let mut remaining = dp.pending_len();
        assert!(dp
            .prepare_retry(&mut remaining, now + Duration::from_secs(3))
            .is_none());
        assert_eq!(dp.pending_counts(), (0, 0));
    }

    #[test]
    fn pending_count_and_byte_limits_are_independent() {
        let mut count_plane = plane();
        let now = Instant::now();
        for _ in 0..=PENDING_LIMIT {
            let mut raw = request();
            raw[9] = 1;
            raw[20] = 8;
            raw[21] = 0;
            raw[22..24].fill(0);
            let icmp_sum = crate::kernel::checksum::checksum(&raw[20..]);
            raw[22..24].copy_from_slice(&icmp_sum.to_be_bytes());
            raw[10..12].fill(0);
            let ip_sum = crate::kernel::checksum::checksum(&raw[..20]);
            raw[10..12].copy_from_slice(&ip_sum.to_be_bytes());
            let ingress = ingress(&count_plane, &raw);
            let prepared = count_plane.prepare(ingress, now).unwrap();
            count_plane.finish(prepared, EgressOutcome::NotReady, now);
        }
        assert_eq!(
            count_plane.pending_counts(),
            (PENDING_LIMIT, PENDING_LIMIT * 28)
        );

        let mut byte_plane = plane();
        let mut accepted_bytes = 0;
        let mut accepted = 0;
        for port in 0..32u16 {
            let mut raw = vec![0; 65535];
            raw[0] = 0x45;
            raw[2..4].copy_from_slice(&65535u16.to_be_bytes());
            raw[8] = 64;
            raw[9] = 17;
            raw[12..16].copy_from_slice(&[10, 77, 0, 2]);
            raw[16..20].copy_from_slice(&[10, 77, 0, 3]);
            raw[20..22].copy_from_slice(&port.to_be_bytes());
            raw[22..24].copy_from_slice(&5678u16.to_be_bytes());
            raw[24..26].copy_from_slice(&65515u16.to_be_bytes());
            let sum = crate::kernel::checksum::checksum(&raw[..20]);
            raw[10..12].copy_from_slice(&sum.to_be_bytes());
            let ingress = ingress(&byte_plane, &raw);
            let prepared = byte_plane.prepare(ingress, now).unwrap();
            byte_plane.finish(prepared, EgressOutcome::NotReady, now);
            if byte_plane.pending_len() == accepted + 1 {
                accepted += 1;
                accepted_bytes += raw.len();
            }
        }
        assert!(accepted < PENDING_LIMIT && accepted_bytes <= PENDING_BYTES);
        assert!(
            accepted_bytes + 65535 > PENDING_BYTES,
            "byte limit, not count limit, rejected the next packet"
        );
        assert_eq!(byte_plane.pending_counts(), (accepted, accepted_bytes));
        assert_eq!(
            byte_plane.flow_counts(),
            (0, 0),
            "NotReady rolls back all active/pending flow and quota state"
        );
    }

    #[test]
    fn retry_batch_budget_counts_discarded_items_and_delivers_following_live_item() {
        let mut dp = plane();
        let now = Instant::now();
        let raw = request();
        let valid_stamp = RouteStamp {
            source_id: "a".into(),
            source_key: dp.config.peers["a"].key_identity(),
            source_ip: dp.config.peers["a"].ip,
            target_id: "b".into(),
            target_key: dp.config.peers["b"].key_identity(),
            target_ip: dp.config.peers["b"].ip,
            origin: RouteOrigin::Direct,
        };
        let live = ingress(&dp, &raw);
        let live_delivery = dp.prepare(live.clone(), now).unwrap();
        dp.finish(live_delivery, EgressOutcome::NotReady, now);

        // Build three independent discard cases ahead of the valid queue item.
        // They are observations only: none owns a flow reservation.
        let expired = PendingPacket {
            ingress: live.clone(),
            stamp: valid_stamp.clone(),
            deadline: now,
        };
        let mut bad_stamp = valid_stamp.clone();
        bad_stamp.target_ip = Ipv4Addr::new(10, 77, 0, 99);
        let bad_provenance = PendingPacket {
            ingress: live.clone(),
            stamp: bad_stamp,
            deadline: now + PENDING_TTL,
        };
        let icmp_raw = ipv4::test_icmp_error(
            Ipv4Addr::new(10, 77, 0, 2),
            Ipv4Addr::new(10, 77, 0, 3),
            3,
            3,
            &raw,
        );
        let icmp_ingress = dp
            .ingress(&dp.authenticated_peer("a").unwrap(), &icmp_raw)
            .unwrap();
        let cannot_prepare = PendingPacket {
            ingress: icmp_ingress,
            stamp: valid_stamp,
            deadline: now + PENDING_TTL,
        };
        for item in [expired, bad_provenance, cannot_prepare].into_iter().rev() {
            dp.pending.bytes += item.ingress.packet.bytes().len();
            dp.pending.entries.push_front(item);
        }

        let mut remaining = 4;
        let delivery = dp
            .prepare_retry(&mut remaining, now + Duration::from_millis(1))
            .expect("the live item must be prepared in the same batch after three discards");
        assert_eq!(remaining, 0, "each popped entry consumes budget");
        assert_eq!(dp.pending_len(), 0);
        assert_eq!(
            delivery.bytes()[8],
            63,
            "retry does not decrement TTL a second time"
        );
        dp.finish(
            delivery,
            EgressOutcome::NotReady,
            now + Duration::from_millis(1),
        );
        assert_eq!(
            dp.pending_counts(),
            (1, raw.len()),
            "NotReady requeues one original copy only"
        );
        assert_eq!(
            dp.flow_counts(),
            (0, 0),
            "retry does not retain flow reservations"
        );
    }

    #[test]
    fn established_tcp_related_icmp_queue_is_cleared_when_initiator_acl_is_revoked() {
        fn tcp(
            src: [u8; 4],
            dst: [u8; 4],
            sport: u16,
            dport: u16,
            flags: u8,
            seq: u32,
            ack: u32,
        ) -> Vec<u8> {
            let mut raw = vec![0; 40];
            raw[0] = 0x45;
            raw[2..4].copy_from_slice(&40u16.to_be_bytes());
            raw[8] = 64;
            raw[9] = 6;
            raw[12..16].copy_from_slice(&src);
            raw[16..20].copy_from_slice(&dst);
            raw[20..22].copy_from_slice(&sport.to_be_bytes());
            raw[22..24].copy_from_slice(&dport.to_be_bytes());
            raw[24..28].copy_from_slice(&seq.to_be_bytes());
            raw[28..32].copy_from_slice(&ack.to_be_bytes());
            raw[32] = 0x50;
            raw[33] = flags;
            let mut pseudo = Vec::new();
            pseudo.extend_from_slice(&src);
            pseudo.extend_from_slice(&dst);
            pseudo.extend_from_slice(&[0, 6]);
            pseudo.extend_from_slice(&20u16.to_be_bytes());
            pseudo.extend_from_slice(&raw[20..]);
            let sum = crate::kernel::checksum::checksum(&pseudo);
            raw[36..38].copy_from_slice(&sum.to_be_bytes());
            let sum = crate::kernel::checksum::checksum(&raw[..20]);
            raw[10..12].copy_from_slice(&sum.to_be_bytes());
            raw
        }
        let mut dp = plane();
        let t0 = Instant::now();
        let a = [10, 77, 0, 2];
        let b = [10, 77, 0, 3];
        let syn = tcp(a, b, 1234, 443, 0x02, 100, 0);
        let syn_delivery = dp.prepare(ingress(&dp, &syn), t0).unwrap();
        dp.finish(syn_delivery, EgressOutcome::Delivered, t0);
        let syn_ack = tcp(b, a, 443, 1234, 0x12, 200, 101);
        let syn_ack_ingress = dp
            .ingress(&dp.authenticated_peer("b").unwrap(), &syn_ack)
            .unwrap();
        let syn_ack_delivery = dp
            .prepare(syn_ack_ingress, t0 + Duration::from_secs(1))
            .unwrap();
        dp.finish(
            syn_ack_delivery,
            EgressOutcome::Delivered,
            t0 + Duration::from_secs(1),
        );
        let ack = tcp(a, b, 1234, 443, 0x10, 101, 201);
        let ack_delivery = dp
            .prepare(ingress(&dp, &ack), t0 + Duration::from_secs(2))
            .unwrap();
        dp.finish(
            ack_delivery,
            EgressOutcome::Delivered,
            t0 + Duration::from_secs(2),
        );
        assert_eq!(dp.flow_counts(), (1, 0));
        let deadline = dp.flows.test_active_observation().unwrap().0;

        // An unrelated topology addition must not invalidate the established mapping.
        let old = dp.config.clone();
        let mut unchanged = old.clone();
        unchanged
            .peers
            .insert("c".into(), peer("c", [10, 77, 0, 4], 3));
        dp.reconcile(&old, unchanged);
        assert_eq!(dp.flow_counts(), (1, 0));

        let icmp = ipv4::test_icmp_error(Ipv4Addr::from(b), Ipv4Addr::from(a), 3, 3, &syn);
        let related = dp
            .ingress(&dp.authenticated_peer("b").unwrap(), &icmp)
            .unwrap();
        let related_delivery = dp
            .prepare(related.clone(), t0 + Duration::from_secs(59))
            .expect("committed TCP mapping admits related ICMP");
        assert_eq!(related_delivery.stamp.origin, RouteOrigin::ReplyOnly);
        assert_eq!(
            related_delivery.bytes()[8],
            63,
            "ICMP TTL is decremented once on ingress"
        );
        dp.finish(
            related_delivery,
            EgressOutcome::NotReady,
            t0 + Duration::from_secs(59),
        );
        assert_eq!(dp.pending_counts(), (1, icmp.len()));
        assert_eq!(
            dp.flows.test_active_observation().unwrap().0,
            deadline,
            "related ICMP does not refresh the established flow deadline"
        );

        let old = dp.config.clone();
        let mut revoked = old.clone();
        let source = revoked.peers.get_mut("a").unwrap();
        source.group = Some(Group {
            id: "g".into(),
            name: "g".into(),
            allowed_groups: vec![],
        });
        dp.reconcile(&old, revoked);
        assert_eq!(
            dp.flow_counts(),
            (0, 0),
            "ACL revocation clears active and pending flow state"
        );
        assert_eq!(
            dp.flows.test_index_counts(),
            (0, 0, 0),
            "ACL revocation clears reverse indexes and quota counts"
        );
        assert_eq!(
            dp.pending_counts(),
            (0, 0),
            "ACL revocation clears queued bytes"
        );
        assert!(
            dp.prepare(related, t0 + Duration::from_secs(60)).is_none(),
            "related ICMP never falls back to the otherwise permitted reverse direct route"
        );
        let mut remaining = 1;
        assert!(
            dp.prepare_retry(&mut remaining, t0 + Duration::from_secs(60))
                .is_none(),
            "expired mapping cannot fall back to a reverse direct route after revoke"
        );
        assert_eq!(remaining, 1);
    }

    #[test]
    fn stateless_icmp_echo_delivery_needs_acl_and_never_creates_flow() {
        let mut dp = plane();
        let now = Instant::now();
        let mut raw = vec![0; 28];
        raw[0] = 0x45;
        raw[2..4].copy_from_slice(&28u16.to_be_bytes());
        raw[8] = 64;
        raw[9] = 1;
        raw[12..16].copy_from_slice(&[10, 77, 0, 2]);
        raw[16..20].copy_from_slice(&[10, 77, 0, 3]);
        raw[20] = 8;
        let sum = crate::kernel::checksum::checksum(&raw[20..]);
        raw[22..24].copy_from_slice(&sum.to_be_bytes());
        let sum = crate::kernel::checksum::checksum(&raw[..20]);
        raw[10..12].copy_from_slice(&sum.to_be_bytes());
        let ingress = ingress(&dp, &raw);
        let delivery = dp
            .prepare(ingress.clone(), now)
            .expect("group ACL permits echo");
        assert!(delivery.reservation.is_none());
        dp.finish(delivery, EgressOutcome::Delivered, now);
        assert_eq!(dp.flow_counts(), (0, 0));
        let old = dp.config.clone();
        let mut new = old.clone();
        new.peers.get_mut("b").unwrap().group = Some(Group {
            id: "other".into(),
            name: "other".into(),
            allowed_groups: vec![],
        });
        new.peers.get_mut("b").unwrap().group_id = "other".into();
        dp.reconcile(&old, new);
        assert!(
            dp.prepare(ingress, now + Duration::from_secs(1)).is_none(),
            "revoked ACL denies stateless echo"
        );
        assert_eq!(dp.flow_counts(), (0, 0));
    }

    #[test]
    fn forward_pending_provenance_changes_are_reconciled_without_retry_or_reservation() {
        for changed in [
            "source ip",
            "target ip",
            "source key",
            "target key",
            "forward id",
            "protocol",
            "port",
            "target",
            "allowlist",
            "acl",
        ] {
            let mut dp = plane();
            let now = Instant::now();
            let hub = Ipv4Addr::new(10, 77, 0, 1);
            dp.config.hub_ip = Some(hub);
            dp.config.forwards = vec![ForwardConfig {
                id: "f".into(),
                protocol: TransportProtocol::Udp,
                target_peer_id: "b".into(),
                target_port: 5678,
                allowed_group_ids: vec!["g".into()],
            }];
            let mut raw = ipv4::test_packet([10, 77, 0, 2], hub.octets(), false, false);
            raw[22..24].copy_from_slice(&5678u16.to_be_bytes());
            raw[10..12].fill(0);
            let sum = crate::kernel::checksum::checksum(&raw[..20]);
            raw[10..12].copy_from_slice(&sum.to_be_bytes());
            let p = ingress(&dp, &raw);
            let prepared = dp.prepare(p, now).unwrap();
            dp.finish(prepared, EgressOutcome::NotReady, now);
            assert_eq!(
                dp.pending_counts().0,
                1,
                "fixture must hold forward pending for {changed}"
            );
            let old = dp.config.clone();
            let mut new = old.clone();
            match changed {
                "source ip" => {
                    new.peers.get_mut("a").unwrap().ip = Ipv4Addr::new(10, 77, 0, 9);
                    new.by_ip.remove(&Ipv4Addr::new(10, 77, 0, 2));
                    new.by_ip.insert(Ipv4Addr::new(10, 77, 0, 9), "a".into());
                }
                "target ip" => {
                    new.peers.get_mut("b").unwrap().ip = Ipv4Addr::new(10, 77, 0, 9);
                    new.by_ip.remove(&Ipv4Addr::new(10, 77, 0, 3));
                    new.by_ip.insert(Ipv4Addr::new(10, 77, 0, 9), "b".into());
                }
                "source key" => new.peers.get_mut("a").unwrap().key = [9; 32],
                "target key" => new.peers.get_mut("b").unwrap().key = [9; 32],
                "acl" => {
                    new.peers.get_mut("a").unwrap().group = Some(Group {
                        id: "g2".into(),
                        name: "g2".into(),
                        allowed_groups: vec![],
                    });
                    new.peers.get_mut("a").unwrap().group_id = "g2".into();
                }
                "forward id" => new.forwards[0].id = "changed".into(),
                "protocol" => new.forwards[0].protocol = TransportProtocol::Tcp,
                "port" => new.forwards[0].target_port = 5679,
                "target" => new.forwards[0].target_peer_id = "a".into(),
                "allowlist" => new.forwards[0].allowed_group_ids.clear(),
                _ => unreachable!(),
            }
            dp.reconcile(&old, new);
            assert_eq!(
                dp.pending_counts(),
                (0, 0),
                "changed {changed} invalidates queued provenance"
            );
            let mut remaining = 1;
            assert!(
                dp.prepare_retry(&mut remaining, now + Duration::from_millis(1))
                    .is_none(),
                "no stale item is retried after {changed}"
            );
            assert_eq!(
                dp.flow_counts(),
                (0, 0),
                "reconcile leaves no reservation after {changed}"
            );
        }
    }

    #[test]
    fn pending_deadline_is_preserved_at_2999ms_and_expires_at_three_seconds() {
        let mut dp = plane();
        let now = Instant::now();
        let raw = request();
        let prepared = dp.prepare(ingress(&dp, &raw), now).unwrap();
        dp.finish(prepared, EgressOutcome::NotReady, now);
        for at in [
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_millis(2999),
        ] {
            let mut remaining = dp.pending_len();
            let retry = dp
                .prepare_retry(&mut remaining, now + at)
                .expect("must remain live before original deadline");
            dp.finish(retry, EgressOutcome::NotReady, now + at);
        }
        assert_eq!(dp.pending_counts(), (1, raw.len()));
        let mut remaining = dp.pending_len();
        assert!(dp
            .prepare_retry(&mut remaining, now + Duration::from_secs(3))
            .is_none());
        assert_eq!(dp.pending_counts(), (0, 0));
    }
}
