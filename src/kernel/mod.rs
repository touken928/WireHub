//! Network kernel: packet protocols, flow state, policy, and WireGuard routing.
mod checksum;
mod flows;
mod ipv4;
mod policy;
mod protocol;
mod router;
pub(crate) mod snapshot;

pub use router::{run_udp, Readiness, ReloadCommand, RuntimeStats, SnapshotLoader};
