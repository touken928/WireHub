# Wirehub

Wirehub 是一个单机运行的 WireGuard 用户态 IPv4 路由服务，带有 Web 管理界面和 HTTP 管理 API。服务器不创建 TUN 设备；客户端使用标准 WireGuard 客户端。当前版本适合实验和受控部署，不应被理解为完整的通用 VPN/网关产品。

原 Go 版本保留在 [`v0`](https://github.com/touken928/WireHub/tree/v0) 分支；`main` 为当前 Rust 版本，使用独立的提交历史。

## 构建

需要 Rust stable/Cargo、Node.js 与 pnpm：

```sh
pnpm --dir frontend install && pnpm --dir frontend build && cargo build --release
```

前端构建产物嵌入 Rust 可执行文件。运行 `target/release/wirehub`。

生成并校验仓库内的 API 定义：

```sh
cargo run --release -- export-openapi > openapi.json.tmp
python3 -m json.tool openapi.json.tmp >/dev/null
mv openapi.json.tmp openapi.json
```

`export-openapi` 仅打印 OpenAPI JSON 后退出，不启动监听器，也不要求设置运行时管理令牌。修改 API 路由/模型后应重新生成并提交 `openapi.json`。

## Docker 镜像发布

向仓库推送 `v*.*.*` 版本 tag 时，[发布工作流](.github/workflows/release.yml) 自动构建并验证 `linux/amd64`、`linux/arm64` 镜像，随后推送到 `ghcr.io/touken928/wirehub`。例如 `v1.0.0` 会生成 `:v1.0.0`、`:1.0.0` 和 `:latest`；预发布版本只生成对应版本标签，不更新 `latest`。Actions 的手动运行只执行两种架构的构建和启动检查。

镜像包含已编译的 Web 界面，以 UID/GID `65532` 运行，数据库与 Hub 密钥保存到 `/data`。在本机访问或受可信反向代理保护的环境中运行：

```sh
docker run --detach --name wirehub \
  --env WIREHUB_ADMIN_TOKEN \
  --env WIREHUB_TRUSTED_PROXY_MODE=1 \
  --publish 127.0.0.1:51820:51820/tcp \
  --publish 51820:51820/udp \
  --volume wirehub-data:/data \
  ghcr.io/touken928/wirehub:latest
```

先按下节设置 `WIREHUB_ADMIN_TOKEN`。镜像内 HTTP 监听 `0.0.0.0`，示例仅将管理端口发布到主机 loopback；如需远程管理，应通过可信的 TLS/认证反向代理访问。使用宿主机目录挂载 `/data` 时，目录应允许 UID/GID `65532` 写入。可用 `docker build -f docker/Dockerfile -t wirehub:local .` 构建本地镜像；用户态集成测试继续直接运行本地进程。

## 启动与配置

`WIREHUB_ADMIN_TOKEN` 必须设置为非空的管理密钥：

```sh
export WIREHUB_ADMIN_TOKEN='replace-with-a-long-random-secret'
./target/release/wirehub
```

可选环境变量：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `WIREHUB_PORT` | `51820` | WireGuard UDP 端口，同时也是 HTTP/TCP 管理服务端口。必须是 1–65535。 |
| `WIREHUB_HTTP_BIND` | `127.0.0.1` | HTTP IPv4 监听地址。默认仅本机访问。 |
| `WIREHUB_DB` | `wirehub.sqlite3` | SQLite 数据库路径。 |
| `WIREHUB_HUB_KEY` | `wirehub.key` | Hub 的 32 字节静态私钥文件路径；不存在时首次启动生成。Unix 上文件权限为 `0600`。 |
| `WIREHUB_TRUSTED_PROXY_MODE` | 未启用 | 仅在 HTTP 监听非 loopback 地址且前面确有可信 TLS/认证反向代理时设为 `1`。 |

**同一个数值端口同时绑定 HTTP/TCP 和 WireGuard/UDP**：默认是 TCP `127.0.0.1:51820` 与 UDP `0.0.0.0:51820`（全部 IPv4 地址）。更改端口后，两种协议一起变化。防火墙/云安全组应允许相应 UDP；TCP 管理界面/API 不应直接暴露到不可信网络。

首次启动时，Wirehub 不会自动选择隧道网络。Web 首次设置默认填写 `10.10.10.0/24`，可在初始化前更改。使用已认证的 `POST /api/setup` 完成一次性设置，JSON 请求体为 `{"subnet":"10.10.10.0/24","endpoint":"vpn.example.com:51820","persistent_keepalive":25}`。`subnet` 必须是规范形式的 RFC1918 IPv4 `/24`；`endpoint` 必须是客户端可访问的主机名或 IPv4 地址加端口（`host:port`，含端口）；`persistent_keepalive` 取值为 `0`–`65535`。设置状态可通过已认证的 `GET /api/setup` 查询：返回 `configured` 和 `settings`（尚未设置时为 `false` 和 `null`）。首次设置后不可再次修改子网。

子网地址分配固定为 `.1` Hub、`.2`–`.254` Peer；`.0` 和 `.255` 保留。Peer 最多 253 个。转发继续共享 Hub 的 `.1` 地址，不分配独立虚拟地址。创建 Peer 时生成的客户端配置仅包含所选子网这一条 `AllowedIPs` 路由。选择的隧道 `/24` 不应与客户端已有 LAN/VPN 路由重叠，否则会发生路由冲突。

`endpoint` 是写入**新生成客户端配置**的公告地址，包含的端口可以不同于本机 `WIREHUB_PORT`/WireGuard UDP 监听端口（例如经端口映射或 NAT 时）；部署者需确保该公告地址和端口从客户端可达，并将其正确转发到服务端 UDP 端口。`PUT /api/settings` 可更新全局 `endpoint` 与 `persistent_keepalive` 默认值，但只影响之后生成的客户端配置，不会重写现有客户端配置；现有客户端必须手动编辑并重新导入配置。客户端私钥只在创建响应中返回一次，服务端不会保存，不能通过更新设置或重新下载配置恢复。

非 loopback HTTP bind 会被拒绝，除非显式设置 `WIREHUB_TRUSTED_PROXY_MODE=1`。该开关本身**不提供** TLS、身份验证、来源限制或代理校验；部署方必须确保可信代理实际终止 TLS、实施认证并保护上游连接，且 Wirehub 的明文 HTTP 监听不可被绕过代理访问。管理 API 通过 `Authorization: Bearer <WIREHUB_ADMIN_TOKEN>` 验证；健康检查 `/api/health` 不要求令牌。不要在浏览器、日志、命令历史或不可信代理中泄露令牌。

Hub 私钥用于确定服务器公钥。丢失或更换密钥会使现有客户端配置失效；备份并限制其访问。客户端创建 API 返回的配置含有新生成的客户端**私钥**，只在创建时提供一次且响应带有 `Cache-Control: no-store`。应立即安全保存并交付给对应用户，勿将响应记录到日志或源码控制；服务端不会把该客户端私钥存入数据库，之后无法重新取回，丢失时请删除并重新配置该 peer。

## 地址与功能边界

- Peer IPv4 地址从首次设置所选 `/24` 的 `.2`–`.254` 分配（253 个地址）；`.1` 始终保留给 Hub，`.0` 和 `.255` 保留。客户端配置恰好包含所选子网的一条 `AllowedIPs` 路由。
- 服务转发共享 Hub 的 `.1` 地址：客户端访问 `<subnet>.1:<target_port>`，流量转发到目标 peer 的 `<peer-ip>:<target_port>`。转发由协议和目标端口唯一标识；同一数值端口可同时配置 TCP 与 UDP，但不能重复创建同协议、同目标端口的转发。转发不创建新的公网监听器，也没有独立的虚拟监听端口。转发请求须同时满足来源 peer 的转发允许列表和来源组到目标组的有向 ACL；TCP NAT idle 映射超时为 300 秒。
- 隧道内 UDP 的新建通信由发起方向授权：直连目标 peer 的 UDP 流量须满足来源组到目标组的有向 ACL；通过 `.1` UDP 转发的流量还须满足来源 peer 的转发允许列表。组 ACL 是有向的，同组 peer 之间也不会自动获得访问权限。对于已经授权并成功发出的 UDP 应用数据报，其精确反向流量会自动获准，不要求另行配置反向组 ACL；反向匹配绑定到已认证 peer 及精确的 IP、协议、源端口和目标端口。直接 ICMP echo 回复仍须有显式的反向 ACL，TCP 行为保持不变。
- UDP 没有可证明业务交付的握手：成功发出一个获准的应用 UDP 数据报才会建立临时反向许可，WireGuard 握手不算业务交付，也不能证明远端应用已收到数据。由于 UDP 无连接，符合完全相同反向 tuple 的包无法区分为应用回复还是其他流量。UDP 临时状态在任一方向最后一次成功业务交付后 60 秒过期；无效或发送失败的流量不会续期，tuple 改变则按新的发起请求重新授权。状态容量有界。管理变更重载的确认是清除活动及待处理 UDP 状态的屏障；重载前已发出的数据报无法撤回。
- 当前声明范围为 IPv4。不要依赖 IPv6 路由、通用 Internet 出口/转发、子网路由、IPv4 分片转发或 hairpin NAT；这些能力未承诺支持。请勿将 `AllowedIPs` 路由范围扩大后推断服务器会提供上述功能。
- 服务端 WireGuard 数据面在进程内通过 userspace 实现，不要求服务器安装 WireGuard 内核模块、创建 TUN 设备或配置内核隧道接口。管理/测试所需的 Linux 客户端仍可使用内核 WireGuard。

## API 与变更注意

OpenAPI 规范位于 [`openapi.json`](openapi.json)，由上述 `export-openapi` 命令从代码生成。API 基路径为 `/api`：

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/health` | 健康状态（无需认证） |
| `GET`, `POST` | `/api/setup` | 查询首次设置状态 / 一次性设置隧道子网、公告端点和 keepalive |
| `PUT` | `/api/settings` | 更新新生成客户端配置使用的全局端点和 keepalive 默认值 |
| `GET`, `POST` | `/api/groups` | 列出/创建组 |
| `DELETE` | `/api/groups/{id}` | 删除组 |
| `PUT` | `/api/groups/{id}/acl` | 设置该组允许访问的目标组 |
| `GET`, `POST` | `/api/peers` | 列出 peer / 创建 peer 并一次性取得客户端配置 |
| `DELETE` | `/api/peers/{id}` | 删除 peer |
| `PUT` | `/api/peers/{id}/group` | 移动 peer 到组 |
| `GET`, `POST` | `/api/forwards` | 列出/创建 TCP 或 UDP 服务转发 |
| `DELETE` | `/api/forwards/{id}` | 删除转发 |

除健康检查外，API 调用都需 bearer token。管理 API 变更先写入 SQLite，再请求数据面重载；重载失败时请求可能返回错误，但数据库更改未必回滚（创建 peer 的特定失败路径除外）。因此遇到超时、`503` 或连接中断时，**不要假设变更未发生，也不要盲目重试非幂等创建**；先用认证后的 GET 查询实际状态，再决定如何修复。变更前备份并评估现有状态。

## 备份、恢复与升级

停止服务后，安全备份 `WIREHUB_HUB_KEY` 指向的密钥文件和 `WIREHUB_DB` 指向的 SQLite 数据库（默认分别为 `wirehub.key`、`wirehub.sqlite3`）。两者应作为同一恢复点保存，并限制备份访问权限。密钥丢失会改变服务端身份；数据库丢失会丢失 peer 公钥、地址、组/ACL 与转发记录。恢复时先恢复这两项，再启动服务。升级前也建议做一致性备份。

**不兼容变更：schema 2 是破坏性升级；所有 schema 1 数据库均会被拒绝，且拒绝时保持原样不变，包括空的 schema 1 数据库。** 不要让新版本直接打开旧数据库，也不要删除或覆盖旧数据来“升级”；不提供迁移。部署 schema 2 时，先停止服务并备份数据库及 Hub 密钥，然后为新版本配置一个全新的、空白数据库路径；按首次设置流程重新配置网络并重新 provision 客户端。旧数据库可保留用于回退/数据参考，但不能作为新版本数据库使用。由于客户端私钥不保存在服务端，重新 provision 会生成新的客户端密钥；需要把新配置安全交付给客户端，并让客户端重新导入。若回退，使用与旧版本匹配的数据库和 Hub 密钥备份。

## 用户态集成测试

[`tests/integration.py`](tests/integration.py) 构建前端、Rust Hub 和独立的 `wireguard-go` netstack 客户端，然后直接启动一个 Hub 与三个本地客户端进程。需要 Rust stable/Cargo、Node.js、pnpm、Go 1.26 和 Python 3；Python 脚本仅使用标准库。从仓库根目录运行：

```sh
python3 tests/integration.py
```

脚本先执行 Go 客户端单元测试，再验证认证/API 与配置隐私、真实 WireGuard 握手、单向 UDP/ICMP ACL、共享 `.1` TCP/UDP 转发、拒绝请求未到达目标（nonce 观测），以及策略重载后的权限撤销。探测流量由独立固定版本的 wireguard-go userspace netstack 发出，不需要 TUN、内核 WireGuard 或 root 网络权限。

每次运行使用随机管理令牌、临时数据库与密钥，并分配独立的本地端口。测试控制 HTTP 仅监听 `127.0.0.1`；Hub 的 UDP 仍按程序行为监听全部 IPv4 地址。测试不修改宿主机路由，结束或中断时停止子进程并删除临时文件。进程日志不会打印，避免泄露令牌或配置。

CI 使用同一脚本，并运行 Rust 测试和已提交 OpenAPI 定义校验。该测试验证用户态实现互通，未覆盖内核客户端或所有生产网络环境。
