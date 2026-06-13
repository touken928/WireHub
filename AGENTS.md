# AGENTS.md — WireHub

WireHub is a single-binary WireGuard hub: userspace `wireguard-go` + gVisor netstack, Gin REST API, SQLite, and a React UI embedded from `internal/static/dist`.

## Start Here

- Entry point: `cmd/wirehub/main.go` → `internal/bootstrap.Run`
- Backend wiring lives in `internal/bootstrap/`; it creates `repo.Store`, `service.App`, `api/http.Server`, and `vpn/runtime.Stack`
- UI source is `web/`; embedded build output is `internal/static/dist`

## Commands That Matter

- Full backend test sweep: `go test ./...`
- Focused package test: `go test ./internal/<pkg>/... -count=1`
- Race-sensitive service checks: `go test ./internal/service/ -race -count=1 -timeout 120s`
- Frontend verification: `cd web && npm run build`
- Production-like build order matters: `cd web && npm ci && npm run build && cd .. && go build -o wirehub ./cmd/wirehub`
- Dev run: `go run ./cmd/wirehub --data-dir ./data`
- Frontend dev expects backend on `:8080`: `go run ./cmd/wirehub --port 8080 --data-dir ./data` and `cd web && npm run dev`

## Verified Repo Conventions

- HTTP handlers in `internal/api/http/handlers/` must call `service.App`; do not access `repo.Store` directly from handlers.
- Register routes only in `internal/api/http/router.go`.
- Password handling must go through `repo.HashPassword` / `repo.VerifyPassword`.
- Add persistence in `internal/repo`, validation/business rules in `internal/domain`, orchestration in `internal/service`, and runtime/network changes in `internal/vpn`.
- When changing user-visible behavior, update both `README.md` and `docs/README_zh.md`.

## Architecture Boundaries

- Keep dependency direction: `cmd -> bootstrap -> {api/http, service, repo, vpn/runtime}`.
- Do not introduce `repo -> api/http` imports or `internal -> cmd` imports.
- `internal/service` is the orchestration layer; avoid duplicating DNS/WireGuard/ACL sync logic in handlers or frontend code.
- `vpn/runtime.Stack` is the live control point for `SyncPortForwards`, `SyncMaps`, reloads, and start/stop.

## Network / Product Gotchas

- Fresh unconfigured instances only allow setup/import from localhost by default; remote first-run setup requires `--allow-remote-setup`.
- Peer configs always use the DB `settings.listen_port`; that does not change the actual host bind port from CLI `--port`.
- WebSocket auth still uses `?token=` because browsers cannot send `Authorization` on upgrade.
- Built-in DNS serves `hub.wirehub`, `{peer}.wirehub`, and `www.*`; bare `wirehub` is not served.

## Testing Reality

- `internal/integration/` contains slower black-box VPN tests.
- `TestPortForwardTCPToPublicAPI` is known flaky / network-dependent; do not treat it as proof your unrelated change failed.
- Backend changes should usually add or update targeted Go tests in the touched package.

## File / Output Hygiene

- Do not commit `data/`, `*.db`, or secrets like `.jwt_secret`.
- If you change frontend code, rebuild before any Go build so `go:embed` picks up fresh assets.
