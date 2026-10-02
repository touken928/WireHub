//! Network kernel: packet protocols, flow state, policy, and WireGuard routing.
mod checksum;
mod flows;
pub(crate) mod dataplane;
mod ipv4;
mod policy;
mod protocol;
mod runtime;
mod wireguard;
pub(crate) mod snapshot;

pub use runtime::{run_udp, Readiness, ReloadCommand, RuntimeStats, SnapshotLoader};
