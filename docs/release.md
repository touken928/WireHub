# Release verification

Current candidate: **1.0.0-rc.1**, schema **5**. The acceptance scope is one Hub,
a private IPv4 /24, Linux servers and the embedded management UI. Mobile testing
is excluded and desktop interoperability is deferred for this acceptance.
Supported behavior and network limitations are in [operations](operations.md).

## Release checklist

- [ ] Run hosted CI on the final commit and retain its artifacts. Confirm native
  Linux amd64/arm64 binaries, both container architectures, startup and embedded UI.
- [ ] Review quality, API/UI, network, operations and endurance results below.
  Use real deployment load and path MTU for production acceptance; loopback
  measurements do not establish WAN throughput or production capacity.
- [ ] Stop the production service, create a verified database/Hub-key pair backup
  and rehearse restore with the exact previous binary/image. Preserve the
  pre-upgrade pair throughout acceptance; see [backup and rollback](operations.md#paired-backup-upgrade-and-rollback).
- [ ] Agree Cargo, lockfile, frontend and exact release-tag versions. CI embeds
  the release commit SHA in binaries and containers and verifies their output.
  RC tags remain prereleases; formal 1.0.0 requires matching `v1.0.0` and packages.

The reusable integration workflow gates publishing. Hosted CI and the two
distribution architectures have not yet been verified for this candidate.

## Local verification

Prerequisites are listed in [operations](operations.md#supported-scope). Build the
embedded frontend before compiling Rust:

```sh
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend build
cargo build --locked
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
pnpm --dir frontend audit --prod --audit-level high
cargo audit
actionlint
pnpm --dir frontend test:ui
WIREHUB_UI_REGRESSIONS=all pnpm --dir frontend test:ui
target/release/wirehub export-openapi > /tmp/wirehub-openapi.json
cmp openapi.json /tmp/wirehub-openapi.json
python3 scripts/check-release.py --tag v1.0.0-rc.1 --binary target/release/wirehub
```

CI also compiles all targets with the minimum supported Rust **1.89.0**.

Python acceptance runners use only the standard library. Shared HTTP, process,
client and report helpers live in `tests/support.py`; each runner keeps its own
scenarios and CLI. Go dependencies are maintained with `go mod tidy`; WireGuard
and x/net are needed for the independent userspace client and its ICMP probes.
Linux kernel acceptance additionally needs Docker and the tools in its fixture
image. Operations backup checks require OpenSSL.

Ruff **0.16.1** is a development-only formatter/linter; it is not needed to run
acceptance. The root `ruff.toml` fixes Python 3.11 syntax, 88-column formatting,
double quotes and import order. CI enforces the same style:

```sh
ruff format tests scripts
ruff check tests scripts
gofmt -w tests/client/main.go tests/client/main_test.go
```

```sh
python3 tests/integration.py
WIREHUB_NETWORK_REGRESSIONS=all python3 tests/integration.py
python3 tests/linux_kernel.py --output /tmp/linux-kernel.json
python3 tests/operations.py --binary target/release/wirehub --output /tmp/operations.json
python3 tests/soak.py --binary target/release/wirehub --seconds 1800 --output /tmp/rc-endurance.json
```

For real previous-version rollback execution, add
`--rollback-binary /absolute/path/to/previous/wirehub` to the operations command.
Without it, the test verifies the restored schema-4 pair without executing an
old binary. For future desktop acceptance, run
`python3 tests/desktop_interop.py --output /tmp/desktop-interop.json` and manually
import/activate its temporary profile in the installed WireGuard client.
Routing benchmark commands and interpretation are in [performance](performance.md).

The network gates use isolated temporary state and processes. Weak-network
acceptance introduces deterministic 5% encrypted data loss, 12 ms delay and
30 ms additional delay every seventh packet, then tests NAT source-port migration,
65-second idle recovery, stream integrity and live revocation. Linux acceptance
uses real kernel WireGuard in disposable Docker network namespaces with
NET_ADMIN/NET_RAW only on the clients, verifying direct/forwarded traffic,
directional ACLs, MTU boundaries, fragmentation rejection and backend arrivals.

Endurance uses two wireguard-go peers, direct/forwarded TCP/UDP, periodic 1 MiB
SHA-256 transfers, unrelated reloads, revoke/regrant, stale-write rejection,
monotonic sampled counters, matching revisions and automatic rekey. Reports
record binary hashes, platform, CPU/RSS and round latency; each round includes
four HTTP-controlled exchanges, not individual packet RTT. CI runs at least
300 seconds; extended CI allows 30-minute or two-hour runs.

## Local acceptance snapshot — 2026-10-04

| Area | Recorded result |
| --- | --- |
| Quality | Formatting, Clippy, actionlint and native Rust 1.89 compilation passed; 187 Rust tests plus one later regression passed; the benchmark is opt-in |
| Dependencies | Rust and production frontend audits found no known vulnerabilities; indirect proc-macro-error 1.0.4 remains unmaintained (RUSTSEC-2024-0370) |
| API/UI | Default and all 23 additional UI scenarios passed; atomic versioned ACLs, conflict drafts, safe rebase, activation recovery, polling and session races covered; authentication precedes extraction and structured errors are non-cacheable |
| Network | Weak-network/NAT/idle/integrity/revocation passed; real Linux kernel WireGuard passed direct/forwarded TCP/UDP, bidirectional ICMP, MTU/fragment boundaries and live revocation |
| Operations | Optimized RC and a real pre-change schema-4 source-baseline binary passed backup/migration/restore/rollback; this is not evidence about an external published old release; bounded SIGTERM and instance-lock exclusion passed |
| Endurance | 1801.641 seconds, 360 rounds, 58 reloads, 9 revoke/regrant cycles, 29 MiB integrity transfers and automatic rekey on both peers; Hub RSS peak 7008 KiB, final 6608 KiB, cumulative CPU 2.34 seconds; round p50 5.25 ms / p99 7.59 ms |
| Performance | 84 release routing/capacity cases passed; plaintext baseline and hardware details in [performance](performance.md) |

## Report retention

Raw JSON reports belong in CI artifacts or release attachments. The four original
local reports are preserved byte-for-byte in the ignored
`artifacts/acceptance/2026-10-04/` directory; this local archive is not distributed
with a repository checkout. Regenerate reports with the commands above and retain
binary/build IDs, hashes, hardware and test scope with the relevant release.
The local Linux run used a temporary classic-builder Dockerfile because its
Buildx connection was unavailable; hosted CI still uses the normal build path.
