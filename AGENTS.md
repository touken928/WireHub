# AGENTS.md — WireHub

Guidance for coding agents working in this repository.

## Project

WireHub is a single-binary WireGuard hub: userspace `wireguard-go` + gVisor netstack on the server, SQLite persistence, Gin REST API, React admin UI embedded via `go:embed`.

- **Module:** `github.com/touken928/wirehub`
- **Entry:** `cmd/wirehub/main.go`
- **User docs:** `README.md` (English), `docs/README_zh.md` (中文)

## Repository layout

```
cmd/wirehub/          # composition root
internal/
  domain/             # pure rules: HubConfig, group ACL, hostnames, port-forward targets, client .conf
  service/            # use cases: Hub (peers, stats poller, VPN attach, ACL sync)
  server/             # HTTP handlers + routes (Gin); embeds *service.Hub
  repo/               # GORM/SQLite persistence
  vpn/                # VPN stack
    stack.go          # lifecycle (was runtime.Network)
    wg/               # WireGuard manager
    dns/              # authoritative *.wirehub DNS on netstack
    filter/           # TUN ACL + gVisor forwarding + tunnel HTTP + hub port proxy (portproxy.go)
  auth/               # JWT login middleware
  password/           # bcrypt helpers (shared by repo + auth; avoids import cycles)
  config/             # CLI flags, defaults, subnet/DNS helpers
  static/             # embedded SPA (built from web/)
  integration/        # black-box Go tests
web/                  # React + Vite + Fluent UI
docs/                 # README_zh.md, assets/
docker/               # Dockerfile, compose
```

**Ignore stale paths** if present locally (`internal/api`, `internal/store`, `internal/app`, `internal/runtime`, etc.) — canonical names are above.

## Dependency direction

```
cmd → server, service, repo, vpn, auth, config, static
server → service, repo, auth, password
service → domain, repo, vpn/wg, vpn/dns
vpn → service, repo, vpn/wg, vpn/dns, vpn/filter
repo → domain, config, password
domain → config, vpn/filter (ACL RuleSet only)
auth → repo, password
```

Do not introduce `repo` → `auth` or `internal` → `cmd` imports.

## Ports and configuration

| Setting | Default | Where |
|--------|---------|--------|
| Hub listen port (TCP Web/API + UDP WireGuard) | `8443` | CLI `--port` (`config.DefaultPort`) |
| Client endpoint port in peer `.conf` | `8443` | DB `settings.listen_port` (`config.DefaultEndpointPort`) |
| DNS domain suffix | `wirehub` | `config.DNSDomain` |
| Hub DNS name | `hub.wirehub` | `config.HubDNSLabel` + suffix |
| Data dir | `./data` | CLI `--data-dir` → `wirehub.db`, `.jwt_secret` |

`settings.listen_port` is for generated client `Endpoint` only. Hub bind/listen uses CLI `--port`.

## Commands

```bash
# Full build (production-like)
cd web && npm ci && npm run build && cd ..
go build -o wirehub ./cmd/wirehub

# Run hub
./wirehub --data-dir ./data
# or: go run ./cmd/wirehub --data-dir ./data

# Backend tests
go test ./...

# Frontend only (proxies /api → :8080; run Go with --port 8080)
cd web && npm run dev
```

Frontend build output: `internal/static/dist` (Vite `outDir`).

## Backend conventions

- **Thin HTTP layer:** `internal/server` handlers delegate peer/network work to `service.Hub` methods; avoid duplicating WG/DNS sync in handlers.
- **Domain logic** stays in `internal/domain` (no GORM, no Gin).
- **Group ACL:** `domain.BuildAccessPolicy` + `GroupLinkPair.Bidirectional` → `vpn/filter.AccessPolicy` (block list + `l4.TransparentTable`) → `wg.Manager.SetAccessPolicy`. Bidirectional: direct WG IP. Unidirectional: `domain.BuildTransparentTable` → TUN transparent relay (`vpn/filter/l4`).
- **Hub L4 (`vpn/filter/l4`):** three modes — **system listen** (DNS `:53`, Web TCP + WG UDP on CLI `--port`), **ForwardProxy** (admin Forward: client dials `hub:listenPort`), **TransparentTable** (uni group link: client dials peer IP:port; hub SNAT on TUN). Shared IPv4 rewrite and ephemeral pool; `l4.ReservedHubPorts(webPort, forwards)` reserves 53, web port, and Forward listen ports from SNAT range via `wg.Manager.ReserveHubPorts`.
- **Hostnames:** `domain.ValidateHostname`, `domain.PeerFQDN` — not a separate package.
- **Port forwards:** `repo.PortForward` → `l4.ForwardProxy`; `vpn.Stack.SyncPortForwards()` after CRUD. API: `handlers_forwards.go`, routes under `/api/forwards`.
- **Group links:** `repo.GroupLink` with `Bidirectional`; directed edges stored as `from_group_id → to_group_id`. UI: link type toolbar bottom-left on Groups canvas. API: `POST/DELETE /api/groups/links`.
- **Passwords:** `password.Hash` / `password.Verify` only; never import `auth` from `repo`.
- **VPN lifecycle:** `vpn.Stack` implements `service.NetworkRuntime` (`SyncPortForwards`, `HubListenPort`); call `service.Hub.AttachNetwork` / `DetachNetwork` from stack start/stop.
- Prefer minimal diffs; match existing naming and file placement.
- Do not commit unless the user asks. Do not commit `data/`, secrets, or local `.db` files.

## Frontend conventions

```
web/src/
  app/           # routes, guards, theme
  api/           # fetch client, types
  components/    # common/, groups/, layout/, peers/
  pages/
  hooks/
  lib/
```

- Path alias: `@/*` → `./src/*` in `tsconfig.app.json` (no `baseUrl`; TS 6 style).
- API base: `/api`; live hub status via WebSocket `GET /api/ws/status?token=…` (replaces polling `/status`). Vite dev proxy enables `ws: true` in `vite.config.ts`.
- UI: Fluent UI v9; groups graph uses `@xyflow/react`.

## Testing notes

- `internal/integration/` spins real netstack/WG paths; can be slow; needs network/syscalls.
- Package tests: `domain`, `repo`, `vpn/dns`, `vpn/filter`, `config`, etc.

## When adding features

1. Model/API types → `repo` + `server` handlers.
2. Orchestration (DB + WG + DNS + ACL) → `service`.
3. Validation / portable config / ACL math → `domain`.
4. New REST routes → `server/router.go` + handler file by area (`handlers_peers.go`, `handlers_groups.go`, `handlers_forwards.go`, …).
5. Update `web/src/api/types.ts` and pages if the UI exposes the feature.
6. Run `go test ./...` and `cd web && npm run build` before finishing.

## Release binaries (CI)

Tag `v*.*.*` triggers `.github/workflows/release.yml`. Targets: `linux-amd64`, `linux-arm64`, `darwin-arm64`, `windows-amd64` (`.exe`). Assets are uncompressed executables named `wirehub-<tag>-<platform>`.

## Docs and README

- Centered HTML header + badges in `README.md` / `docs/README_zh.md`.
- Architecture diagram: Mermaid in **How it works** / **工作原理**.
- Screenshot: `docs/assets/screenshot.png`.
