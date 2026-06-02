# WireHub

**[中文文档](README_zh.md)**

Centralized hub-and-spoke WireGuard management. One hub with a public IP manages every peer through a web dashboard.

## Features

- **Hub-and-spoke topology** — only the hub needs a routable endpoint; peers connect outbound
- **Web admin UI** — React + Fluent UI; embedded in the release binary
- **Peer lifecycle** — create, edit, disable, delete; export `.conf` or scan a QR code
- **Built-in DNS** — `{name}.wirehub.internal`; `www.{name}.wirehub.internal` is an alias (`www` → hub)
- **Hostname access control** — gitignore-style exclude rules enforced at the IP layer between peers
- **Live status** — last handshake, RX/TX bytes, network usage charts
- **Userspace WireGuard** — [wireguard-go](https://github.com/WireGuard/wireguard-go) + gVisor netstack; no kernel module on the hub

## How it works

```
                         Public Internet
                               │
                    UDP/TCP :8443 (same port)
                               │
                    ┌──────────▼──────────┐
  Admin browser ──► │  Web UI + REST API  │
                    │  (Gin + React)      │
                    └──────────┬──────────┘
                               │
              ┌────────────────▼────────────────┐
              │            WireHub Hub            │
              │   wireguard-go · userspace TUN    │
              │   DNS (UDP 53) · access filter    │
              └────────────────┬──────────────────┘
                               │ WireGuard tunnel
           ┌───────────────────┼───────────────────┐
           │                   │                   │
      Laptop peer         Server peer        Restricted peer
     (full mesh)          (full mesh)         (exclude rules)
```

After setup, the hub listens on its VPN address for **TCP (web UI)** and **UDP 53 (DNS)**. Other services you run on peer machines are reached over the tunnel; peer-to-peer traffic is L3-forwarded and filtered by exclude rules. Traffic to the hub itself is not subject to peer exclude rules.

## Requirements

| Component | Version |
|-----------|---------|
| Go (from source) | 1.26+ |
| Node.js (frontend build) | 22+ |
| Docker (optional) | 20+ |

## Quick start

### Docker (recommended)

Pull a release image from GitHub Container Registry:

```bash
docker pull ghcr.io/touken928/wirehub:latest

docker run -d --name wirehub \
  -p 8443:8443 -p 8443:8443/udp \
  -v wirehub-data:/app/data \
  ghcr.io/touken928/wirehub:latest
```

Build locally with Compose:

```bash
docker compose -f docker/docker-compose.yml up -d --build
```

Open **http://localhost:8443/setup** and complete the first-run wizard.

No `--cap-add` or `--privileged` is required: WireHub uses wireguard-go's userspace netstack (gVisor), not a kernel TUN device. CLI flags (`--data-dir`, `--port`, `--bind`) are optional in the image — defaults are `./data` (i.e. `/app/data` with `WORKDIR /app`), `8443`, and `0.0.0.0`.

### Pre-built binary

Download the archive for your platform from [GitHub Releases](https://github.com/touken928/wirehub/releases), then:

```bash
tar -xzf wirehub-vX.Y.Z-linux-amd64.tar.gz
./wirehub-linux-amd64 --data-dir ./data
```

Supported release targets: **Linux amd64**, **Linux arm64**, **macOS arm64**.

### From source

```bash
cd web && npm ci && npm run build && cd ..
go build -o wirehub ./cmd/wirehub
./wirehub --data-dir ./data
```

The frontend build output is written to `internal/static/dist` and embedded via `go:embed` in `internal/static/static.go`.

## First-run setup

On a fresh install the HTTP server starts immediately, but WireGuard and DNS start only after setup.

1. Open **http://&lt;host&gt;:8443/setup**
2. Fill in the wizard fields below
3. Sign in with the admin account you created

| Field | Default | Notes |
|-------|---------|-------|
| Endpoint | *(required)* | Public IP or hostname clients use in `Endpoint = …:8443` |
| Subnet | `100.127.0.0/24` | VPN CIDR; hub and DNS always use the first host address (`.1`) |
| Admin username | `admin` | Stored in SQLite |
| Admin password | *(required)* | Stored as bcrypt hash |
| MTU | `1420` | Applied to generated client configs |
| Status interval | `1` | Seconds between peer status polls |
| Additional DNS | `1.2.4.8`, `1.1.1.1` | Upstream resolvers in client configs; external queries are forwarded by the hub |

These values are persisted in SQLite and **cannot be changed in the UI** after setup. Use **Reset** in the dashboard to wipe all hub data and return to setup mode.

The JWT signing secret is created automatically on first launch and stored at `{data-dir}/.jwt_secret`.

## CLI flags

Process-level settings only — not stored in the database:

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | `8443` | TCP (web UI) and UDP (WireGuard) share this port |
| `--bind` | `0.0.0.0` | Address for the HTTP server |
| `--data-dir` | `./data` | SQLite database, JWT secret, persistent state |

```bash
./wirehub --port 8443 --bind 0.0.0.0 --data-dir ./data
```

## Client setup

1. Sign in to the web UI
2. Under **Users**, add a peer
3. Download the `.conf` file or scan the QR code
4. Import into any WireGuard client and connect

Each client config includes the hub endpoint, keys, allowed IPs, DNS (`hub IP` plus additional resolvers from setup), and MTU.

## DNS

WireHub runs a resolver on the hub VPN IP (UDP 53). Names under `wirehub.internal` are answered authoritatively; other queries are forwarded to the **additional DNS** servers configured at setup (default `1.2.4.8`, `1.1.1.1`).

Client WireGuard configs list DNS as `{hub_ip}, {upstream…}` so peers resolve internal hostnames via the hub and can reach the public internet through upstream resolvers.

| Name | Resolves to |
|------|-------------|
| `hub.wirehub.internal` | Hub VPN IP |
| `www.wirehub.internal` | Hub VPN IP (alias) |
| `{peer}.wirehub.internal` | Peer VPN IP |
| `www.{peer}.wirehub.internal` | Peer VPN IP (alias) |

The suffix `wirehub.internal` is fixed (`internal/config/config.go`). Avoid `.local` on macOS — the OS treats it as mDNS.

## Access control

By default every peer can reach every other peer. Per-user **exclude rules** restrict peer-to-peer connectivity using hostname patterns:

- One pattern per line; `#` starts a comment
- Exact names (`alice`) or wildcards (`server-*`)
- Prefix `!` to re-allow after a broader match (last matching rule wins)
- Hostnames only — no domain suffix, no raw IPs
- Cannot exclude your own hostname

Rules are enforced in the hub's userspace forwarding path. They apply to **peer ↔ peer** traffic, not to reaching the hub web UI or DNS.

## Development

**Backend + embedded UI** (production-like):

```bash
cd web && npm ci && npm run build && cd ..
go run ./cmd/wirehub --data-dir ./data
```

**Frontend dev server** with API proxy (run the Go server on port `8080`):

```bash
# terminal 1
go run ./cmd/wirehub --port 8080 --data-dir ./data

# terminal 2
cd web && npm ci && npm run dev
```

Vite proxies `/api` to `http://localhost:8080` (see `web/vite.config.ts`).

**Tests:**

```bash
go test ./...
```

## License

[GNU General Public License v3.0](LICENSE)
