# WireHub

**[English](README.md)**

集中式 Hub-and-Spoke WireGuard 管理平台。只需一台拥有公网 IP 的 Hub，即可通过 Web 控制台管理所有客户端。

## 功能

- **星型拓扑** — 仅 Hub 需要可路由的公网 Endpoint，各 Peer 主动连出
- **Web 管理界面** — React + Fluent UI，发布版内嵌于单一二进制
- **Peer 全生命周期** — 创建、编辑、禁用、删除；导出 `.conf` 或扫码导入
- **内置 DNS** — `{name}.wirehub.internal`；`www.{name}.wirehub.internal` 为别名（`www` 指向 Hub）
- **主机名访问控制** — 类 gitignore 的排除规则，在 IP 层限制 Peer 间互通
- **在线状态** — 最近握手时间、收发流量、用量图表
- **用户态 WireGuard** — [wireguard-go](https://github.com/WireGuard/wireguard-go) + gVisor netstack，Hub 侧无需内核模块

## 工作原理

```
                         公网
                           │
                UDP/TCP 共用 :8443
                           │
                ┌──────────▼──────────┐
  管理浏览器 ──► │  Web UI + REST API  │
                │  (Gin + React)      │
                └──────────┬──────────┘
                           │
            ┌──────────────▼──────────────┐
            │          WireHub Hub        │
            │  wireguard-go · 用户态 TUN   │
            │  DNS (UDP 53) · 访问过滤     │
            └──────────────┬──────────────┘
                           │ WireGuard 隧道
           ┌───────────────┼───────────────┐
           │               │               │
      笔记本 Peer      服务器 Peer      受限 Peer
     （默认可互通）   （默认可互通）   （排除规则）
```

完成初始化后，Hub 在其 VPN 地址上监听 **TCP（Web UI）** 和 **UDP 53（DNS）**。Peer 上运行的其他服务通过隧道访问；Peer 之间的流量由 Hub 做 L3 转发，并按排除规则过滤。访问 Hub 自身（Web、DNS）不受 Peer 排除规则影响。

## 环境要求

| 组件 | 版本 |
|------|------|
| Go（源码构建） | 1.26+ |
| Node.js（前端构建） | 22+ |
| Docker（可选） | 20+ |

## 快速开始

### Docker（推荐）

从 GitHub Container Registry 拉取发布镜像：

```bash
docker pull ghcr.io/touken928/wirehub:latest

docker run -d --name wirehub \
  -p 8443:8443 -p 8443:8443/udp \
  -v wirehub-data:/app/data \
  ghcr.io/touken928/wirehub:latest
```

本地用 Compose 构建：

```bash
docker compose -f docker/docker-compose.yml up -d --build
```

浏览器打开 **http://localhost:8443/setup**，完成首次配置向导。

无需 `--cap-add` 或 `--privileged`：WireHub 使用 wireguard-go 的用户态 netstack（gVisor），不依赖内核 TUN 设备。镜像内 CLI 参数（`--data-dir`、`--port`、`--bind`）可省略，默认分别为 `./data`（`WORKDIR /app` 下即 `/app/data`）、`8443`、`0.0.0.0`。

### 预编译二进制

在 [GitHub Releases](https://github.com/touken928/wirehub/releases) 下载对应平台压缩包：

```bash
tar -xzf wirehub-vX.Y.Z-linux-amd64.tar.gz
./wirehub-linux-amd64 --data-dir ./data
```

发布目标：**Linux amd64**、**Linux arm64**、**macOS arm64**。

### 从源码构建

```bash
cd web && npm ci && npm run build && cd ..
go build -o wirehub ./cmd/wirehub
./wirehub --data-dir ./data
```

前端产物输出到 `internal/static/dist`，由 `internal/static/static.go` 通过 `go:embed` 嵌入二进制。

## 首次配置

全新安装时 HTTP 服务会立即启动，WireGuard 与 DNS 仅在完成配置后才会启动。

1. 打开 **http://&lt;主机&gt;:8443/setup**
2. 填写下表字段
3. 使用创建的管理员账号登录

| 字段 | 默认值 | 说明 |
|------|--------|------|
| Endpoint | *必填* | 客户端 `Endpoint = …:8443` 使用的公网 IP 或域名 |
| Subnet | `100.127.0.0/24` | VPN 网段；Hub 与 DNS 固定为首个主机地址（`.1`） |
| 管理员用户名 | `admin` | 存入 SQLite |
| 管理员密码 | *必填* | bcrypt 哈希存储 |
| MTU | `1420` | 写入生成的客户端配置 |
| 状态轮询间隔 | `1` | Peer 状态刷新间隔（秒） |
| 额外 DNS | `1.2.4.8`、`1.1.1.1` | 写入客户端配置的公共解析器；Hub 会转发外网 DNS 查询 |

以上配置持久化在 SQLite 中，**初始化后无法在 UI 内修改**。可在控制台使用 **Reset** 清空全部数据并重新进入配置向导。

JWT 签名密钥在首次启动时自动生成，保存在 `{data-dir}/.jwt_secret`。

## 命令行参数

仅影响进程运行，不写入数据库：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--port` | `8443` | TCP（Web）与 UDP（WireGuard）共用端口 |
| `--bind` | `0.0.0.0` | HTTP 监听地址 |
| `--data-dir` | `./data` | SQLite、JWT 密钥及持久化数据目录 |

```bash
./wirehub --port 8443 --bind 0.0.0.0 --data-dir ./data
```

## 客户端接入

1. 登录 Web 控制台
2. 在 **Users** 中添加 Peer
3. 下载 `.conf` 或扫描二维码
4. 导入任意 WireGuard 客户端并连接

生成的配置包含 Hub Endpoint、密钥、AllowedIPs、DNS（Hub IP + 额外解析器）及 MTU 等参数。

## DNS

WireHub 在 Hub VPN IP 上提供 DNS 服务（UDP 53）。`wirehub.internal` 下的名称由 Hub 权威解析；其他查询会转发到首次配置时设置的**额外 DNS**（默认 `1.2.4.8`、`1.1.1.1`）。

客户端 WireGuard 配置中的 DNS 为 `{hub_ip}, {upstream…}`，Peer 经 Hub 解析内网主机名，并通过上游解析器访问公网。

| 域名 | 解析结果 |
|------|----------|
| `hub.wirehub.internal` | Hub VPN IP |
| `www.wirehub.internal` | Hub VPN IP（别名） |
| `{peer}.wirehub.internal` | 对应 Peer VPN IP |
| `www.{peer}.wirehub.internal` | 对应 Peer VPN IP（别名） |

域名后缀固定为 `wirehub.internal`（见 `internal/config/config.go`）。macOS 上请避免使用 `.local`，系统会将其视为 mDNS。

## 访问控制

默认所有 Peer 可互相访问。可为每个用户配置 **排除规则（exclude rules）**，按主机名模式限制 Peer 间连通：

- 每行一条规则；`#` 开头为注释
- 精确匹配（`alice`）或通配符（`server-*`）
- 前缀 `!` 表示在更宽泛规则之后重新允许（**最后匹配的规则生效**）
- 仅主机名，不要写域名后缀或 IP
- 不能排除自己的主机名

规则在 Hub 用户态转发路径中执行，仅作用于 **Peer ↔ Peer** 流量，不影响访问 Hub Web 或 DNS。

## 开发

**后端 + 内嵌 UI**（接近生产环境）：

```bash
cd web && npm ci && npm run build && cd ..
go run ./cmd/wirehub --data-dir ./data
```

**前端热更新**（API 代理，Go 服务需监听 `8080`）：

```bash
# 终端 1
go run ./cmd/wirehub --port 8080 --data-dir ./data

# 终端 2
cd web && npm ci && npm run dev
```

Vite 将 `/api` 代理到 `http://localhost:8080`（见 `web/vite.config.ts`）。

**测试：**

```bash
go test ./...
```

## 许可证

[GNU General Public License v3.0](LICENSE)
