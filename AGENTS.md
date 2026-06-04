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
cmd/wirehub/              # composition root (main)
internal/
  domain/                 # pure rules: ACL, hostnames, forwards, client .conf
  service/                # use cases: Hub (peers, poller, VPN attach, ACL sync)
  server/                 # Gin handlers + routes; wraps *service.Hub
  repo/                   # GORM/SQLite persistence
  vpn/
    stack.go              # VPN lifecycle (NetworkRuntime)
    wg/                   # WireGuard manager
    dns/                  # authoritative *.wirehub on netstack
    filter/               # TUN ACL, gVisor forwarding, tunnel HTTP
      l4/                 # system listen, ForwardProxy, TransparentTable (SNAT)
  auth/                   # JWT login middleware
  password/               # bcrypt (shared by repo + auth; no import cycles)
  config/                 # CLI flags, defaults, subnet/DNS helpers
  ws/                     # WebSocket hub for live status broadcast
  static/                 # go:embed SPA (built from web/)
  integration/            # black-box Go tests (slow; needs syscalls)
web/                      # React + Vite + Fluent UI v9
docs/                     # README_zh.md, assets/
docker/                   # Dockerfile, compose.yml
```

**Ignore stale paths** if present locally (`internal/api`, `internal/store`, `internal/app`, `internal/runtime`, `internal/http`, `internal/dns`, `internal/wg`, `internal/network`, `internal/hostname`, etc.). Canonical packages are listed above.

## Architecture

### Dependency direction

```
cmd → server, service, repo, vpn, auth, config, static
server → service, repo, auth, password, ws
service → domain, repo, vpn/wg, vpn/dns
vpn → service, repo, vpn/wg, vpn/dns, vpn/filter
repo → domain, config, password
domain → config, vpn/filter (ACL types only)
auth → repo, password
```

**Do not introduce** `repo → auth` or `internal → cmd` imports.

### Layer responsibilities

| Layer | Role |
|-------|------|
| `cmd/wirehub` | Wire repo, server, auth, VPN stack; start HTTP; start VPN when configured |
| `server` | Thin HTTP: validate input, call `service.Hub`, return JSON. No WG/DNS logic in handlers |
| `service` | Orchestration: DB + WG + DNS + ACL sync, peer lifecycle, status poller |
| `domain` | Portable rules and validation. No GORM, no Gin |
| `repo` | Models, queries, setup/import/export |
| `vpn` | Data plane: netstack, WireGuard, DNS server, ACL filter, L4 proxies |

### Startup sequence

1. Parse CLI flags (`config.ParseFlags`).
2. Open SQLite (`repo.New`).
3. Create `server.Server` + `auth.Service`.
4. Create `vpn.Stack`, register as `service.NetworkRuntime`.
5. Register Gin routes + embed static SPA.
6. If hub is configured → `stack.Start()` (WG, DNS, filter, forwards). Else log `/setup` URL only.

### Network stack (`internal/vpn`)

**Control plane vs data plane**

- Control plane: Gin REST + React UI (JWT). Live status via WebSocket (`GET /api/ws/status?token=…`).
- Data plane: WireGuard tunnels terminate in netstack. Peer-to-peer traffic is forwarded and ACL-filtered. Traffic to the hub itself (Web, DNS, forward listeners) is not subject to peer ACL rules.

**L4 modes (`vpn/filter/l4`)**

| Mode | Purpose |
|------|---------|
| System listen | DNS `:53`, Web TCP + WG UDP on CLI `--port` |
| ForwardProxy | Admin **Forward** rules: peer dials `hub_ip:listen_port` → target host:port |
| TransparentTable | Unidirectional group link: peer dials target peer IP:port; hub SNAT on TUN |

Shared IPv4 rewrite and ephemeral port pool. `l4.ReservedHubPorts(webPort, forwards)` reserves port 53, web port, and Forward listen ports from the SNAT range via `wg.Manager.ReserveHubPorts`.

**Group ACL**

- `domain.BuildAccessPolicy(peers, links)` → `vpn/filter.AccessPolicy` (block list + `l4.TransparentTable`) → `wg.Manager.SetAccessPolicy`.
- Same group: direct WireGuard IP connectivity.
- Cross group: default deny; explicit link on Groups graph required.
- Bidirectional link: both groups may initiate to each other.
- Unidirectional link (`A → B`): `domain.BuildTransparentTable` → TUN transparent relay in `vpn/filter/l4`.

**DNS**

- Suffix: `wirehub` (`config.DNSDomain`). Hub label: `hub` → `hub.wirehub`.
- `hub.wirehub`, `{peer}.wirehub`, and `www.*` aliases are authoritative on hub VPN IP (UDP 53).
- Bare `wirehub` / `www.wirehub` are not served. When upstream resolvers are configured in settings, other names forward server-side; with none, external names are refused.
- Peer `.conf` `DNS` line: hub VPN IP only (`repo.Settings.ClientDNS()`). Upstream resolvers are not pushed to clients.

**Port forwards**

- Model: `repo.PortForward` → runtime `l4.ForwardProxy`.
- After CRUD: `vpn.Stack.SyncPortForwards()`.
- Handlers: `internal/server/handlers_forwards.go`, routes under `/api/forwards`.

**Group links**

- Model: `repo.GroupLink` with `Bidirectional`; stored as directed `from_group_id → to_group_id`.
- Handlers: `handlers_groups.go`. Routes: `POST/DELETE /api/groups/links`.
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

### HTTP handlers (`internal/server`)

- Delegate peer/network work to `service.Hub` methods.
- One handler file per area: `handlers_peers.go`, `handlers_groups.go`, `handlers_forwards.go`, `handlers_settings.go`, `handlers_setup.go`, `handlers_ws.go`.
- Register routes in `router.go` only.
- WebSocket auth uses query param `?token=` (browsers cannot set `Authorization` on upgrade).

### Domain (`internal/domain`)

- Hostnames: `ValidateHostname`, `PeerFQDN`, `HubFQDN` — keep in `domain`, not a separate package.
- Forward targets: `ValidateForwardTargetHost` in `domain/forward.go`.
- Client configs: `domain` template helpers.

### Passwords

- Use `password.Hash` / `password.Verify` only.
- Never import `auth` from `repo` (cycle).

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
  pages/            # Dashboard, Groups, Peers, Forward, Settings, Login, Setup
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
| Integration | `internal/integration/` | Real netstack/WG; slow; needs network/syscalls |
| Unit packages | `domain`, `repo`, `vpn/dns`, `vpn/filter`, `config`, … | Fast; run targeted tests when touching one area |

Add or update tests when changing behavior in `domain`, `repo`, or `vpn/filter`. Integration tests cover DNS, forwards, unidirectional links, peers.

Frontend: run `cd web && npm run build` to catch TypeScript and bundle errors.

## Adding features

1. **Model / persistence** → `internal/repo` (models, queries).
2. **Validation / portable logic** → `internal/domain`.
3. **Orchestration** (DB + WG + DNS + ACL) → `internal/service`.
4. **HTTP** → handler file + route in `internal/server/router.go`.
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
