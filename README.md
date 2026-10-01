<h1 align="center">WireHub</h1>

<p align="center">
  Connect your devices and manage your WireGuard network, group access, and service forwarding in your browser.
</p>

<p align="center">
  <a href="https://github.com/touken928/WireHub/actions/workflows/integration.yml"><img src="https://img.shields.io/github/actions/workflow/status/touken928/WireHub/integration.yml?branch=main&style=for-the-badge&label=CI" alt="CI"></a>
  <a href="https://github.com/touken928/WireHub/pkgs/container/wirehub"><img src="https://img.shields.io/badge/Docker-GHCR-2496ED?style=for-the-badge&logo=docker&logoColor=white" alt="Docker image"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=for-the-badge" alt="GPL-3.0"></a>
</p>

<p align="center">
  <img src="docs/assets/overview.png" alt="WireHub dashboard" width="960">
</p>

## Features

- View peer activity and traffic.
- Add peers and download their WireGuard configurations.
- Connect groups by dragging to set one-way, two-way, and same-group access.
- Share peer services with selected groups through a single hub address.

## Quick start

### Docker

Replace the admin token with your own long random secret, then start WireHub:

```sh
export WIREHUB_ADMIN_TOKEN='replace-with-a-long-random-secret'

docker run -d --name wirehub \
  --restart unless-stopped \
  -e WIREHUB_ADMIN_TOKEN \
  -e WIREHUB_TRUSTED_PROXY_MODE=1 \
  -p 127.0.0.1:51820:51820/tcp \
  -p 51820:51820/udp \
  -v wirehub-data:/data \
  ghcr.io/touken928/wirehub:latest
```

### Linux / Windows binary

Download the file for your platform from [GitHub Releases](https://github.com/touken928/WireHub/releases):

| Platform | File |
| --- | --- |
| Linux amd64 | `wirehub-vX.Y.Z-linux-amd64` |
| Linux arm64 | `wirehub-vX.Y.Z-linux-arm64` |
| Windows amd64 | `wirehub-vX.Y.Z-windows-amd64.exe` |

**Linux**

```sh
chmod +x wirehub-vX.Y.Z-linux-amd64
export WIREHUB_ADMIN_TOKEN='replace-with-a-long-random-secret'
./wirehub-vX.Y.Z-linux-amd64
```

Use the `linux-arm64` file on ARM servers.

**Windows (PowerShell)**

```powershell
$env:WIREHUB_ADMIN_TOKEN = 'replace-with-a-long-random-secret'
.\wirehub-vX.Y.Z-windows-amd64.exe
```

The web UI is included. Run the binary from a dedicated folder and keep that folder's data when updating.

Open **[http://localhost:51820](http://localhost:51820)**, enter the same admin token, and click **Connect**.

For a remote server, run `ssh -L 51820:127.0.0.1:51820 user@server`, then open the address above in your local browser. The admin interface is available only on the server's loopback interface by default. Use an HTTPS reverse proxy for public access.

### Initial setup

| Field | Value |
| --- | --- |
| **Subnet** | Defaults to `10.10.10.0/24`. Choose a range that does not overlap your existing networks. |
| **Endpoint** | Your server's public IP or domain with a port, such as `vpn.example.com:51820`. |
| **Keepalive** | Defaults to `25` seconds. |

Click **Create network** to finish setup. The subnet cannot be changed afterward. With the default subnet, the hub uses `10.10.10.1` and peers start at `10.10.10.2`. Allow UDP port `51820` on your server.

## Usage

### Add a peer

1. Open **Groups** and click **New group**.
2. Open **Peers**, click **New peer**, enter a name, and select a group.
3. Download the generated `.conf`, import it into a [WireGuard client](https://www.wireguard.com/install/), and connect.

Save the configuration when you create the peer; it is only provided once. If you lose it, delete and recreate the peer.

### Set access permissions

In **Groups**, drag from a connection handle on one group to another:

- **One way**: allow the source group to access the target group.
- **Both ways**: allow both groups to access each other.
- **Intra-group access**: enable this in the group details to allow peers in the same group to access each other.

Click **Save** to apply changes. Select a connection and press **Delete** to remove access. Access between peers is denied until explicitly allowed.

### Forward a service

In **Forwards**, click **New forward** and choose the target peer, TCP/UDP, service port, and allowed groups. Each source group also needs access to the target group in **Groups**.

For example, forward a peer's TCP `8080` service so authorized peers can reach it at **`10.10.10.1:8080`**.

### Settings and updates

Change Endpoint and Keepalive in **Settings**. Changes apply to configurations generated afterward.

To update, pull the image, stop and remove the old container, then repeat the startup command with the same `wirehub-data` volume. Keep and back up this volume to preserve your network configuration.

---

Each version tag publishes **Linux amd64 / arm64** and **Windows amd64** binaries to GitHub Releases, along with **amd64 / arm64** Docker images. See [`v0`](https://github.com/touken928/WireHub/tree/v0) for the previous version.
# Database and hub-key backups

This release uses strict schema version 3. Older or structurally drifted databases are rejected; there is no automatic migration. At startup, WireHub binds the hub private-key file to the public identity persisted in SQLite; network setup does not create or bind that identity. Back up the SQLite database and hub private-key file together as an immutable identity pair. Restoring only one half can make startup fail because the persisted public identity must match the private key. Schema drift detection includes unexpected SQLite statistics tables and index objects.
