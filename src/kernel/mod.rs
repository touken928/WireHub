//! Network kernel: packet protocols, flow state, policy, and WireGuard routing.
mod checksum;
pub(crate) mod control;
pub(crate) mod dataplane;
mod flows;
mod ipv4;
mod policy;
mod protocol;
mod runtime;
pub(crate) mod snapshot;
mod wireguard;

pub use control::KernelHandle;
pub(crate) use control::ReloadPermit;
#[expect(
    unused_imports,
    reason = "public kernel API facade re-exports statistics and reload error types"
)]
pub use control::{PeerRuntimeStats, ReloadError};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "public kernel API facade re-exports initialization error type"
    )
)]
pub use runtime::StartError;
pub use runtime::{Kernel, RunError, SnapshotLoadError};
pub use snapshot::CompiledSnapshot;

#[cfg(test)]
mod benchmark;
