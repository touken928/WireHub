//! Network kernel: packet protocols, flow state, policy, and WireGuard routing.
pub(crate) mod config;
mod checksum;
mod flows;
mod ipv4;
mod policy;
mod protocol;
mod router;

pub use router::{run_udp, Readiness, ReloadCommand, RuntimeStats};
