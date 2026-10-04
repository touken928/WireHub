# Operations

The working version is **1.0.0-rc.1**, with schema 5. This is a release candidate;
local evidence does not establish a published 1.0.0 release or a hosted CI pass.
See [release verification](release.md) and [performance measurements](performance.md).

## Supported scope

| Surface | Support / evidence |
| --- | --- |
| Server | One hub, private IPv4 /24, assigned /32 peers; no subnet changes after setup |
| Native distribution | Linux amd64 and arm64, native musl release builds in CI |
| Container | Linux amd64 and arm64, non-root UID/GID 65532; no host TUN required |
| Development | Unix only; macOS arm64 build and local integration verified |
| Windows / IPv6 | Unsupported |
| WireGuard clients | Standard generated configuration, MTU 1420; real Linux kernel acceptance recorded |
| Desktop | macOS WireGuard 1.0.16 installed; acceptance deferred by user; no desktop pass claimed |
| Mobile | Outside this acceptance scope at the user's request |
| Filesystem | Trusted local directory with stable locking, hard links and directory fsync; network filesystems unsupported |

Build prerequisites: Rust **1.89+**, Node.js 22, pnpm 9.15.9, native C toolchain.
Test utilities additionally require Python **3.11+**, Go 1.26, OpenSSL with X25519,
Chrome/Playwright and Docker for the real Linux kernel acceptance.

## Network compatibility

Generated configurations use `MTU = 1420`. The router accepts complete IPv4
packets, including DF, and rejects all fragments: first (MF), middle (MF + offset)
and final (offset). It does not reassemble fragments, fragment outgoing packets
or generate ICMP fragmentation-needed messages.

TCP endpoints should segment to their interface MTU. At an inner MTU of 1420,
UDP payloads can be at most **1392 bytes** without IPv4 options. On paths with
additional encapsulation, apply a smaller explicit client MTU such as 1280 and
verify the actual path. Existing client configurations must be updated manually.

ICMP Echo is stateless and requires ACLs for both request and reply directions.
TCP/UDP replies use committed flow mappings; related ICMP errors use those
mappings without creating or refreshing flows. WireGuard keepalives retain
transport reachability but do not refresh application-flow idle timers. TCP
application heartbeats or TCP keepalives can retain established flows, subject
to capacity limits and immediate acknowledged policy revocation.

## Configuration

| Variable | Default | Contract |
| --- | --- | --- |
| `WIREHUB_ADMIN_TOKEN` | none | Required, nonempty bearer secret; generate and store securely |
| `WIREHUB_PORT` | `51820` | Integer 1–65535; same numeric port for UDP tunnel and TCP HTTP |
| `WIREHUB_HTTP_BIND` | `127.0.0.1` | IPv4 address; non-loopback requires proxy mode |
| `WIREHUB_TRUSTED_PROXY_MODE` | `0` | Only literal `1` enables non-loopback HTTP; does not itself provide TLS or proxy authentication |
| `WIREHUB_DB` | `wirehub.sqlite3` | Local regular database; one service instance per canonical path |
| `WIREHUB_HUB_KEY` | `wirehub.key` | Raw 32-byte private key, regular non-symlink file, exactly mode 0600 |
| `WIREHUB_BUILD_ID` | Git commit or `unknown` | Build-time only; release CI sets the full release commit SHA |

The container overrides bind to `0.0.0.0` and the two state paths to `/data/...`;
it still requires proxy mode and an admin token. Mount persistent `/data`, and
allow at least **10 seconds** for stop grace (`docker stop --time 10 wirehub`).
SIGINT/SIGTERM stop new HTTP requests and drain existing requests for at most
8 seconds while the UDP router remains available. Then UDP stops. A deadline
expiry can interrupt outstanding responses; check persisted state before retrying.

Metadata commands never create or migrate state:

```sh
wirehub --version
wirehub --help
wirehub export-openapi > /tmp/wirehub-openapi.json
```

## HTTPS proxy deployment

For a native service keep HTTP on `127.0.0.1:51820`. An Nginx example (replace
the domain and install the corresponding TLS certificate/key):

```nginx
server {
    listen 443 ssl;
    server_name vpn.example.com;
    ssl_certificate /etc/letsencrypt/live/vpn.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/vpn.example.com/privkey.pem;
    location / {
        proxy_pass http://127.0.0.1:51820;
        proxy_set_header Host $host;
        proxy_set_header Authorization $http_authorization;
        proxy_connect_timeout 5s;
        proxy_read_timeout 30s;
        proxy_send_timeout 30s;
    }
}
```

The browser still supplies the WireHub bearer token. Restrict public access to
TCP 443 and UDP 51820; keep upstream TCP 51820 accessible only to the trusted
proxy. When container HTTP is published on host loopback, enable proxy mode
inside the container as in the README. Setting proxy mode alone is insufficient
to protect a publicly published HTTP port. UDP goes directly to the Hub, not
through this HTTP proxy. Forwarded headers do not replace bearer authentication.

For systemd use a dedicated account, a mode-0700 state directory, absolute state
paths and a protected environment file. A minimal unit:

```ini
[Unit]
Description=WireHub
After=network.target

[Service]
User=wirehub
Group=wirehub
WorkingDirectory=/var/lib/wirehub
EnvironmentFile=/etc/wirehub/environment
ExecStart=/usr/local/bin/wirehub serve
Restart=on-failure
TimeoutStopSec=10
UMask=0077
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

## Paired backup, upgrade and rollback

Stop the service before backup. Keep the database, hub key and service lock in
their original trusted directory; never delete `.wirehub.lock` to bypass an active
instance. Keep **both** halves of the identity pair. SQLite WAL contents are
included through the SQLite backup API; a plain copy of a live main DB is insufficient.

Run the repository utility as the service account, with a new backup destination:

```sh
python3 scripts/state-backup.py backup \
  --database /var/lib/wirehub/wirehub.sqlite3 \
  --key /var/lib/wirehub/wirehub.key \
  --output /secure-backups/wirehub-before-upgrade
python3 scripts/state-backup.py verify /secure-backups/wirehub-before-upgrade
```

The destination must not exist. The utility checks the instance lock, SQLite
integrity and matching public identity, writes mode-0600 files, then writes a
durable manifest with checksums, schema version and revision. Protect the archive
as a private key; it is not encrypted by this utility. Preserve an immutable
pre-upgrade archive plus the exact old binary/image version and build ID.

Schema 5 adds the monotonic configuration revision. Canonical schemas **3 and 4**
are validated and migrated atomically to 5 on startup. Drifted or unsupported
schemas are rejected. For an upgrade, verify the new binary version and the
pre-upgrade pair, install the binary, start once and check `/api/ready` plus
authenticated `/api/status`, then check saved inventory and traffic. Preserve
the archive throughout acceptance.

For rollback stop the new service and restore the **pre-upgrade pair into a new
directory**:

```sh
python3 scripts/state-backup.py restore \
  --source /secure-backups/wirehub-before-upgrade \
  --output /var/lib/wirehub-restored
```

Set both state paths to that directory, preserve service-account ownership and
key mode 0600, then start the recorded old binary. An old schema-4 binary cannot
open schema 5; replacing only the executable is insufficient. Rollback discards
configuration changes made since the archive, so preserve the stopped upgraded
pair separately if those changes need review. Do not edit schema numbers to
force compatibility. Do not replace live files or overwrite existing archives.

The backup helper refuses backup while the Rust service holds the instance lock.
Use the [operations acceptance command](release.md#local-verification) to exercise
temporary-pair backup, identity mismatch rejection, migration and rollback before
an upgrade.

## API stability

[OpenAPI](../openapi.json) documents bearer security, JSON errors and revision
preconditions. Management endpoints authenticate before extracting request bodies.
All `/api` responses use `Cache-Control: no-store`; public health/readiness checks
return `{ "ok": true/false }`. Other errors use:

```json
{"code":"revision_conflict","message":"Configuration changed. Reload and review before saving.","persisted_revision":12}
```

| Status / code | Handling |
| --- | --- |
| 401 `unauthorized` | Supply a valid `Authorization: Bearer …` |
| 400 `invalid_request`, `invalid_policy`, `invalid_revision`, `invalid_reference` | Correct input; no invalid write is committed |
| 404 `not_found`, `group_not_found` | Refresh inventory |
| 409 `revision_conflict` | Reload one `/api/config` snapshot; review preserved edits before saving |
| 409 `conflict`, `resource_in_use`, `setup_required`, `already_configured`, `address_pool_exhausted` | Resolve inventory/setup conflict |
| 413 / 415 / 422 | Body too large / JSON content type missing / schema mismatch |
| 428 `revision_required` | Legacy ACL API requires quoted `If-Match` revision |
| 500 `database_error`, `internal_error` | Inspect protected service logs and storage |
| 503 `runtime_unavailable` | Runtime reservation failed before this write; inspect status |
| 503 `activation_unconfirmed` | Saved write may be active later; read configuration and status before retrying |

`PUT /api/policy` accepts `expected_revision` and the entire batch of group ACL
changes. It commits once or rejects the batch. Successful responses include the
committed revision and installed revision. Legacy `PUT /api/groups/{id}/acl`
requires `If-Match: "12"` from `/api/config`; it uses the same atomic path.
For batch/settings saves the returned failure revision is transaction-specific.
For legacy inventory mutations an activation failure exposes the currently
observed persisted revision, which can include a concurrent later write.
Peer creation uses separate durable provisioning/cleanup; its 503 response can
indicate cleanup uncertainty. Inspect peer inventory before retrying. A lost
successful peer response requires deleting and recreating the peer to obtain a
new one-time private configuration.

## Troubleshooting

| Symptom | Check / action |
| --- | --- |
| Startup rejects HTTP bind | Keep loopback or configure a protected TLS proxy and literal proxy mode `1` |
| Database already in use | Stop the owning instance; do not remove its lock |
| Key identity/mode error | Restore the matching pair and mode 0600; never generate a replacement key over existing state |
| Schema error after rollback | Restore the original pre-upgrade pair, not just the old executable |
| Healthy HTTP but no forwarding | Check `/api/ready`, `/api/status`, handshake, group direction, source forward grant and destination service |
| Save conflict | Review fresh server policy; explicitly reapply or discard the draft |
| Save returns 503 | Check persisted/installed revisions before retrying; the UI blocks edits until activation is confirmed |
| Large UDP fails | Stay within path MTU; fragments are rejected, see [network compatibility](#network-compatibility) |
| Idle application stops | Use application/TCP heartbeats; WireGuard keepalive does not retain flow state |

For reproducible network, operations and endurance checks, see
[release verification](release.md#local-verification).
