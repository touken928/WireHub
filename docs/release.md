# Release verification

Release target: **1.2.0**, schema **6**. The acceptance scope is one Hub,
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
  RC tags remain prereleases; formal releases require exact agreement between the tag and packages.

The reusable integration workflow gates publishing. The
[hosted RC preflight](https://github.com/touken928/WireHub/actions/runs/37187394418)
passed on commit `3fcd5a2`: quality, minimum Rust, UI, real Linux kernel and
userspace network checks, operations, five-minute endurance, Rust tests,
performance and OpenAPI agreement. Native Linux amd64/arm64 binaries and both
container architectures also passed version/build-ID, startup and embedded-UI
checks. Its reports and binary artifacts are retained with that workflow run.
The tag-triggered release workflow repeats the gates on the exact tagged commit
before publishing binaries, checksums, containers and a GitHub release (prerelease for RC tags).

## 1.2.0 release

- Select the Hub entry port independently of the target port, for example
  `10.10.10.1:8080` → `10.10.10.4:8000`, for both TCP and UDP.
- Reserve each Hub port per protocol, allowing multiple entries to the same
  backend service and TCP/UDP entries with the same numeric port.
- Edit either port in place; activation removes affected active and queued NAT
  mappings. Replies retain the correct Hub entry port.
- Display forwards as aligned horizontal rows, with separate entry and target
  addresses, compact access groups and responsive controls.
- Upgrade canonical schemas 3–5 atomically to 6, preserving existing forward
  entry ports, configuration revisions and Hub identity. Legacy API requests
  without `listen_port` continue to use `target_port` for both sides.
- Extend paired-backup support to schema 6. Rollback to v1.1.0 requires restoring
  the verified pre-upgrade database/key pair.

Local verification passed 199 Rust tests (one opt-in benchmark ignored), Clippy,
production UI regression and responsive layout checks, real userspace and Linux
kernel WireGuard traffic with different entry/target ports, live entry-port
edits, schema 5→6 paired backup/restore with a v1.1.0 source-baseline binary,
schema 6 backup/restore, and five-minute endurance with automatic rekey.
The exact tagged commit must pass all hosted gates before publication.

The Linux-kernel TCP integrity gate sends and verifies 4 MiB on a single
connection under a 60-second total deadline; small TCP probes retain a
5-second budget. Progress never extends the total deadline, and the gate does
not retry failed transfers. Failure reports distinguish connection errors,
timeouts, premature EOF and byte mismatches, recording sent/received counts and
elapsed time without packet contents or keys. CI uploads this evidence on both
success and failure. Loopback regression tests cover delayed replies, trickling
progress, corruption and truncation alongside the kernel interoperability job.

## 1.1.0 release

- Edit forwards in place: name, protocol, port, target peer and allowed groups.
  Updates require a configuration revision and wait for runtime activation.
- Authorize forwarded services independently through their allowed groups.
  Direct peer access still requires a Groups ACL. Removing a forward grant
  revokes active and queued forwarded traffic; changing direct ACLs does not.
- Simplify Groups to canvas-based links and compact member details.
- Isolate forward form state, fix duplicate cards during refresh/create races,
  and keep keyboard focus inside dialogs while loading.
- Simplify reply routing and index forward rules once per flow reconciliation.
- Preserve database schema 5. Review existing forward allowlists before upgrade:
  their service grants now apply even when direct group access is denied.

Local verification passed 196 Rust tests (one opt-in benchmark ignored), Clippy,
full UI regression, real userspace and Linux-kernel WireGuard checks, HTTP
content integrity, operations acceptance and five-minute endurance. The final
release commit must pass the complete hosted workflow before publishing.

## 1.0.1 patch release

- Remove the non-loopback HTTP startup restriction and the proxy-mode environment
  variable. IPv4 bind validation and required admin-token authentication remain.
- Provide a Compose deployment using the pinned 1.0.1 image and `./data:/data`,
  with a single command to prepare the non-root container's data directory.
- Exercise non-loopback startup without an additional opt-in in operations
  acceptance and remove the old variable from container acceptance workflows.
- Keep API behavior and database schema 5 unchanged from 1.0.0.

The tag-triggered workflow must pass all gates on the exact release commit before
publishing. The previous formal release passed its
[release workflow](https://github.com/touken928/WireHub/actions/runs/38892124110).

## 1.0.0 release review — 2026-10-16

The formal release includes the post-RC bounded pending-packet retries,
target-specific authenticated drains, interrupted provisioning cleanup retries,
and paired-backup support for atomically published hard-linked Hub keys.
Review covered those changes, authentication and API cache behavior, default-deny
routing, startup recovery, and the publication dependency graph.

The release workflow now waits for both native binary builds before publishing
container tags. Package manifests and the checked-in OpenAPI version agree on
1.0.0. Local Rust verification passed 193 tests (one opt-in benchmark ignored),
and the production frontend build passed. The exact tagged commit must still pass
all hosted integration, audit, architecture and startup gates before publication;
prior RC results alone do not establish formal-release acceptance.

Production backup/restore and real-deployment capacity acceptance remain operator
responsibilities. Desktop/mobile acceptance remains outside the supported scope.

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
target/release/wirehub export-openapi > /tmp/wirehub-openapi.json
cmp openapi.json /tmp/wirehub-openapi.json
python3 scripts/check-release.py --tag v1.2.0 --binary target/release/wirehub
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
python3 tests/linux_kernel.py --output /tmp/linux-kernel.json
python3 tests/operations.py --binary target/release/wirehub --output /tmp/operations.json
python3 tests/soak.py --binary target/release/wirehub --seconds 1800 --output /tmp/rc-endurance.json
```

For real previous-version rollback execution, add
`--rollback-binary /absolute/path/to/previous/wirehub` to the operations command.
Without it, the test verifies the restored schema-5 pair without executing an
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

Test environment switches are not part of the deployment configuration.
The normal integration command always includes weak-network and 65-second idle
recovery; the normal UI command runs the core suite and all regression scenarios
in Chrome, saving screenshots and failure diagnostics under `artifacts/ui/`.
The routing benchmark uses a fixed 50,000 iterations per case. Internal child
process credentials and lock-test coordination are managed by the test runners.

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
Buildx connection was unavailable; the hosted RC preflight passed with the normal
Buildx path. Hosted operations acceptance restored the schema-4 pair without
executing an old binary; the local source-baseline rollback evidence remains
distinct from acceptance against an external published previous release.
