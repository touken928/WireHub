use crate::{
    kernel::snapshot::{ForwardConfig, PeerConfig, PeerConfigView},
    model::Group,
};
use std::net::Ipv4Addr;

/// Borrowed authorization context, independent of WireGuard tunnel state.
#[derive(Clone, Copy)]
pub(super) struct PeerPolicy<'a> {
    pub peer: &'a PeerConfig,
    pub group: Option<&'a Group>,
}

pub(super) fn route_allowed(source: Option<&Group>, destination: Option<&Group>) -> bool {
    matches!((source, destination), (Some(s), Some(d)) if allows(s, d))
}

/// Directed, default-deny group policy. Same-group traffic is denied unless explicitly allowed.
pub fn allows(source: &Group, destination: &Group) -> bool {
    source.allowed_groups.iter().any(|id| id == &destination.id)
}

/// Service access is authorized independently of direct peer routing.
/// Authentication and assigned source-address validation happen before routing.
pub fn forward_allowed(forward: &ForwardConfig, source_group_id: &str) -> bool {
    forward
        .allowed_group_ids
        .iter()
        .any(|id| id == source_group_id)
}

/// A peer may only originate packets from its assigned /32 address.
pub fn source_is_valid(peer: &impl PeerConfigView, source: Ipv4Addr) -> bool {
    peer.ip() == source
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Forward, Peer};
    #[test]
    fn forward_authorization_is_independent_of_direct_group_acl() {
        let mut forward: ForwardConfig = Forward {
            id: "f".into(),
            name: "f".into(),
            protocol: "udp".into(),
            target_peer_id: "b".into(),
            target_port: 53,
            allowed_group_ids: vec!["a".into()],
        }
        .into();
        assert!(forward_allowed(&forward, "a"));
        assert!(!forward_allowed(&forward, "other"));
        forward.allowed_group_ids.clear();
        assert!(!forward_allowed(&forward, "a"));
    }
    #[test]
    fn acl_is_directed_and_default_deny() {
        let a = Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec!["b".into()],
        };
        let b = Group {
            id: "b".into(),
            name: "B".into(),
            allowed_groups: vec![],
        };
        assert!(allows(&a, &b));
        assert!(!allows(&b, &a));
        assert!(!allows(&a, &a));
    }
    #[test]
    fn validates_exact_peer_source() {
        let p = Peer {
            id: "p".into(),
            name: "P".into(),
            public_key: "".into(),
            ipv4: "10.0.0.2".into(),
            group_id: "g".into(),
            received_bytes: 0,
            sent_bytes: 0,
            last_handshake_unix: None,
        };
        assert!(source_is_valid(&p, "10.0.0.2".parse().unwrap()));
        assert!(!source_is_valid(&p, "10.0.0.3".parse().unwrap()));
    }
}
