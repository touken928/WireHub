# AGENTS.md — WireHub

Instructions for AI coding agents working in this repository. User-facing docs live in `README.md` and `docs/README_zh.md`; this file covers build steps, architecture, and conventions agents must follow.

## Project overview

WireHub is a single-binary WireGuard hub: userspace `wireguard-go` + gVisor netstack on the server, SQLite persistence, Gin REST API, React admin UI embedded via `go:embed`.

| Item | Value |
|------|-------|
| Go module | `github.com/touken928/wirehub` |
| Entry point | `cmd/wirehub/main.go` |
| Go version | 1.26+ |
| Node.js (frontend build) | 22+ |
| User docs | `README.md` (EN), `docs/README_zh.md` (ZH) |

Hub-and-spoke topology: only the hub needs a public endpoint; peers connect outbound. After setup the hub runs WireGuard, authoritative `*.wirehub` DNS, group-based ACL, optional L4 port forwards, and Web/API on the VPN address (`hub.wirehub`).

## Repository layout

```
cmd/wirehub/              # delegates to internal/bootstrap
internal/
  bootstrap/              # composition root: repo, App, Stack, Gin, VPN start
  api/
    http/                 # Gin REST (handlers/, dto/, httputil/, auth/); calls service.App only
    ws/                   # WebSocket status transport
  domain/                 # portable rules (subpackages: peer, policy, forward, map, runtime, client, hub)
  service/                # App + Hub; LoadSyncBundle; peers/groups/forwards/maps/settings/setup/status
  repo/                   # GORM/SQLite + password.go (bcrypt helpers)
  vpn/
    runtime/              # Stack lifecycle (Dataplane); no repo import
    tunnel/               # WireGuard + netstack TUN
    tun/                  # TUN ACL + conntrack
    snat/                 # unidirectional group-link transparent SNAT
    ingress/              # ForwardProxy, MapProxy, tunnel Web
    dns/                  # authoritative *.wirehub (in-memory catalog)
    policy/               # domain.AccessPolicySpec → tun.AccessPolicy
    netstack/             # gVisor stack helpers, IP forwarding
    core/                 # shared constants (hub ports)
  config/                 # CLI flags, defaults, subnet/DNS helpers
  static/                 # go:embed SPA (built from web/)
  integration/            # VPN black-box tests: mesh.go/sync.go harness + *_test.go by feature
web/                      # React + Vite + Fluent UI v9
docs/                     # README_zh.md, assets/
docker/                   # Dockerfile, compose.yml
```

**Ignore stale paths** if present locally (`internal/api`, `internal/store`, `internal/app`, `internal/runtime`, `internal/http`, `internal/dns`, `internal/wg`, `internal/network`, `internal/hostname`, etc.). Canonical packages are listed above.

## Architecture

### Dependency direction

```
cmd → bootstrap
bootstrap → api/http, service, repo, vpn/runtime, config, static
api/http → service, repo (dto + VerifyPassword in handlers), api/http/auth, api/ws, vpn/core
api/http/auth → repo (JWT + Gin middleware); login rate limit in api/http/httputil
api/ws → (snapshot via service.Status)
service → domain/*, repo, vpn/runtime, vpn/tunnel (keygen only)
vpn/runtime → domain/runtime, domain/policy, vpn/* (Callbacks = service.App)
vpn/* → domain/*, config, other vpn subpackages only
repo → domain/*, config
domain/* → config (and domain subpackages only)
```

**Do not introduce** `repo → api/http` or `internal → cmd` imports.

### Layer responsibilities

| Layer | Role |
|-------|------|
| `cmd/wirehub` | Parse flags; call `bootstrap.Run` |
| `bootstrap` | Wire repo, `service.App`, `api/http`, Stack, routes, optional `stack.Start(bundle)` |
| `api/http` | Thin HTTP: validate input, call `service.App`, map via `dto/` |
| `service` | `App` (Store + `Hub`); use cases in `peers.go`, `groups.go`, …; `LoadSyncBundle` |
| `domain/*` | Portable rules; `domain/runtime.SyncBundle`, `domain/policy.AccessPolicySpec`, … |
| `repo` | Models, queries, setup/import/export |
| `vpn/runtime` | Data plane `Stack` / `Dataplane` |

### Startup sequence

1. Parse CLI flags (`config.ParseFlags`).
2. `bootstrap.Run`: open SQLite, create `service.App` + `api/http.Server`, `runtime.Stack`, register routes.
3. If hub is configured → `app.LoadSyncBundle()` then `stack.Start(bundle)`.
4. Else log `/setup` URL only.

### Network stack (`internal/vpn`)

**Control plane vs data plane**

- Control plane: Gin REST + React UI (JWT). Live status via WebSocket (`GET /api/ws/status?token=…`).
- Data plane: WireGuard tunnels terminate in netstack. Peer-to-peer traffic is forwarded and ACL-filtered. Traffic to the hub itself (Web, DNS, forward listeners) is not subject to peer ACL rules.

**L4 modes (`vpn/ingress` + `vpn/snat`)**

| Mode | Package | Purpose |
|------|---------|---------|
| System listen | `dns`, `ingress` | DNS `:53`, tunnel Web TCP `:80` on hub VPN IP; WG UDP on host (CLI `--port`) |
| ForwardProxy | `ingress` | Admin **Forward**: `hub_ip:listen_port` → target |
| MapProxy | `ingress` | Admin **Maps**: `{slug}.wirehub` → VIP; same-port TCP/UDP |
| TransparentTable | `snat` + `tun` | Unidirectional group links; hub SNAT on TUN |

`ingress.ReservedHubPorts` + `tunnel.Manager.ReserveHubPorts` keep SNAT ephemeral ports away from hub listeners.

**Group ACL**

- `domain/policy.BuildAccessPolicySpec(...)` → `vpn/policy.Apply(spec)` → `tunnel.Manager.SetAccessPolicy`.
- Control plane pushes `domain/runtime.SyncBundle` via `App.LoadSyncBundle()`; runtime applies peers, DNS catalog, ingress, policy.
- Map VIPs: blocked on TUN when peer’s group not in `MapGroupAllow`; DNS NXDOMAIN; `MapProxy` rejects disallowed traffic.
- `repo.PeerGroup.AllowIntraGroup` (default `true`, JSON `allow_intra_group`) — UI label **Same-group interconnect** on Groups detail panel.
- Same group: direct WireGuard IP connectivity when `AllowIntraGroup` is true; peers blocked from each other when false (hub Web/DNS/forwards still reachable).
- Cross group: default deny; explicit link on Groups graph required.
- Bidirectional link: both groups may initiate to each other.
- Unidirectional link (`A → B`): `domain/policy.BuildTransparentSpec` → `vpn/policy.Apply` → `vpn/snat` on TUN.

**DNS**

- Suffix: `wirehub` (`config.DNSDomain`). Hub label: `hub` → `hub.wirehub`.
- `hub.wirehub`, `{peer}.wirehub`, and `www.*` aliases are authoritative on hub VPN IP (UDP 53).
- Bare `wirehub` / `www.wirehub` are not served. When upstream resolvers are configured in settings, other names forward server-side; with none, external names are refused.
- Peer `.conf` `DNS` line: hub VPN IP only (`repo.Settings.ClientDNS()`). Upstream resolvers are not pushed to clients.

**Port forwards**

- Model: `repo.PortForward` → `ingress.ForwardProxy` via `SyncBundle.Forwards`.
- After CRUD: `vpn.Stack.SyncPortForwards()`.
- Handlers: `internal/api/http/handlers/forwards.go`, routes under `/api/forwards`.

**Service maps**

- Model: `repo.ServiceMap`, `repo.MapGroupAllow` → `ingress.MapProxy` + VIP on netstack NIC via `SyncBundle.Maps`.
- After CRUD: `vpn.Stack.SyncMaps()` + `SyncAccessFilter()`.
- Handlers: `internal/api/http/handlers/maps.go`, routes under `/api/maps`.
- Validation: `domain.ValidateMapSlug`, `domain.ValidateMapGroupIDs` (≥1 group).

**Group links**

- Model: `repo.GroupLink` with `Bidirectional`; stored as directed `from_group_id → to_group_id`.
- Handlers: `internal/api/http/handlers/groups.go`. Routes: `POST/DELETE /api/groups/links`.
- UI: icon-only toolbar on Groups canvas (`LinkDrawToolbar`).

## Configuration

| Setting | Default | Source |
|---------|---------|--------|
| Hub listen port (TCP Web/API + UDP WireGuard) | `8443` | CLI `--port` (`config.DefaultPort`) |
| Client `Endpoint` port in peer `.conf` | `8443` | DB `settings.listen_port` (`config.DefaultEndpointPort`) |
| HTTP bind | `0.0.0.0` | CLI `--bind` |
| Data directory | `./data` | CLI `--data-dir` → `wirehub.db`, `.jwt_secret` |
| DNS suffix | `wirehub` | `config.DNSDomain` |
| Hub DNS name | `hub.wirehub` | `config.HubDNSLabel` + suffix |
| Upstream DNS | — (optional) | DB `settings.upstream_dns`; hub forwards external queries when set |

`settings.listen_port` is written to generated peer configs only. It does **not** change the hub bind port (CLI `--port`).

## Commands

Run these before finishing backend or full-stack changes:

```bash
# Production-like build (frontend must be built before go build embeds it)
cd web && npm ci && npm run build && cd ..
go build -o wirehub ./cmd/wirehub

# Run hub
go run ./cmd/wirehub --data-dir ./data

# Backend tests (integration tests are slow)
go test ./...

# Frontend dev (terminal 1: Go on 8080; terminal 2: Vite proxies /api + ws)
go run ./cmd/wirehub --port 8080 --data-dir ./data
cd web && npm run dev
```

Frontend build output: `internal/static/dist` (Vite `outDir`). Run `npm run build` before `go build` / release so `go:embed` picks up fresh assets.

Docker local build: `docker compose -f docker/compose.yml up -d --build`.

## Backend conventions

### HTTP handlers (`internal/api/http`)

- Handlers live in `handlers/`; call `service.App` only (no direct `repo.Store` in handlers).
- JSON shapes in `dto/`; path/query helpers in `httputil/`.
- Register routes in `internal/api/http/router.go` only.
- WebSocket auth uses query param `?token=` (browsers cannot set `Authorization` on upgrade).

### Domain (`internal/domain`)

- Hostnames: `ValidateHostname`, `PeerFQDN`, `HubFQDN` — keep in `domain`, not a separate package.
- Forward targets: `ValidateForwardTargetHost` in `domain/forward.go`.
- Client configs: `domain` template helpers.

### Passwords

- Use `repo.HashPassword` / `repo.VerifyPassword` only (see `repo/password.go`).

### VPN lifecycle

- `vpn.Stack` implements `service.NetworkRuntime` (`SyncPortForwards`, `HubListenPort`, etc.).
- `service.Hub.AttachNetwork` / `DetachNetwork` called from stack start/stop.
- Status poller publishes via `StatusPublisher` → `internal/ws.Hub`.

### General

- Prefer minimal diffs; match existing naming and file placement.
- Do not commit unless the user asks.
- Do not commit `data/`, secrets, or local `.db` files.

## Frontend conventions

### Directory layout

```
web/src/
  app/              # routes, guards, ThemeProvider, StatusProvider, SetupStatusProvider
  api/              # fetch client, types (types.ts)
  ws/               # WebSocket status client (status.ts)
  components/
    auth/           # AuthField
    common/         # charts, confirm dialogs
    groups/         # React Flow graph, LinkDrawToolbar, GroupDetailPanel
    layout/         # AppLayout, LoginLayout, PageHeader
    peers/          # PeerMemberCard, CreatePeerDialog, ConfigDialog
  pages/            # Dashboard, Groups, Peers, Forward, Maps, Settings, Login, Setup
  hooks/
  lib/
  styles/           # loginPage, layout, pageLayout
```

### Routes (`web/src/app/routes.tsx`)

| Path | Page |
|------|------|
| `/setup` | Setup (new hub or import DB) |
| `/login` | Login |
| `/` | Dashboard |
| `/groups` | Groups graph |
| `/peers` | Peers list |
| `/forward` | Port forwards |
| `/settings` | Settings |

Auth shell: `LoginLayout` + `styles/loginPage.ts` shared by Login and Setup.

### API and realtime

- REST base: `/api`.
- Types: `web/src/api/types.ts` — update when adding API fields.
- Path alias: `@/*` → `./src/*` in `tsconfig.app.json` (no `baseUrl`).
- Live status: WebSocket `/api/ws/status?token=…` via `StatusProvider` (not polling).
- Vite dev proxy: `/api` → `localhost:8080` with `ws: true` (`vite.config.ts`).

### UI

- Fluent UI v9.
- Groups topology: `@xyflow/react`.
- Theme toggle: icon only (sun/moon) in `AppLayout`.

## Testing

| Scope | Command | Notes |
|-------|---------|-------|
| All packages | `go test ./...` | Required before finishing backend changes |
| Integration | `internal/integration/` | Harness in `mesh.go`/`sync.go`/…; scenarios in `dns_test.go`, `forward_test.go`, …; external forward tests need network |
| Unit packages | `domain`, `repo`, `vpn/dns`, `vpn/filter`, `config`, … | Fast; run targeted tests when touching one area |

Add or update tests when changing behavior in `domain`, `repo`, or `vpn/filter`. Integration tests cover DNS, forwards, unidirectional links, peers.

Frontend: run `cd web && npm run build` to catch TypeScript and bundle errors.

## Adding features

1. **Model / persistence** → `internal/repo` (models, queries).
2. **Validation / portable logic** → `internal/domain`.
3. **Orchestration** (DB + WG + DNS + ACL) → `internal/service`.
4. **HTTP** → `internal/api/http/handlers/*.go` + route in `internal/api/http/router.go`.
5. **UI** → `web/src/api/types.ts`, relevant page/component.
6. **Verify** → `go test ./...` and `cd web && npm run build`.

Do not duplicate WG/DNS/ACL sync in handlers or pages; call `service.Hub` methods.

## User documentation

When changing user-visible behavior, update `README.md` and `docs/README_zh.md` together (EN/ZH parity).

| README section | EN | ZH |
|----------------|----|----|
| Intro | Centered tagline under title | Same |
| Features | Bullet list | 功能 |
| Architecture | Mermaid diagram | 架构 |
| Screenshot | `docs/assets/screenshot.png` | Same |

Do not move agent-only detail (dependency rules, handler file map, L4 internals) into README.

## Release

Tag `v*.*.*` triggers `.github/workflows/release.yml`.

| Asset | Format |
|-------|--------|
| Binaries | `wirehub-<tag>-<platform>` (uncompressed) |
| Platforms | `linux-amd64`, `linux-arm64`, `darwin-arm64`, `windows-amd64` (`.exe`) |
| Docker | `ghcr.io/touken928/wirehub:<tag>` and `latest` |
