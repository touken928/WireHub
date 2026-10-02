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

/// A forward requires both its source-group allowlist and the backend's
/// directed ACL to permit the source group.
pub fn forward_allowed(
    forward: &ForwardConfig,
    source_group_id: &str,
    source: Option<&Group>,
    target_group_id: &str,
) -> bool {
    forward
        .allowed_group_ids
        .iter()
        .any(|id| id == source_group_id)
        && source.map_or(false, |group| {
            group.allowed_groups.iter().any(|id| id == target_group_id)
        })
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
    fn forward_requires_allowlist_and_backend_acl() {
        let forward = Forward {
            id: "f".into(),
            name: "f".into(),
            protocol: "udp".into(),
            target_peer_id: "b".into(),
            target_port: 53,
            allowed_group_ids: vec!["a".into()],
        };
        let source = Group {
            id: "a".into(),
            name: "A".into(),
            allowed_groups: vec!["b".into()],
        };
        assert!(forward_allowed(
            &forward.clone().into(),
            "a",
            Some(&source),
            "b"
        ));
        assert!(!forward_allowed(
            &forward.clone().into(),
            "c",
            Some(&source),
            "b"
        ));
        let denied = Group {
            allowed_groups: vec![],
            ..source
        };
        assert!(!forward_allowed(&forward.into(), "a", Some(&denied), "b"));
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
