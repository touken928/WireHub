<h1 align="center">WireHub</h1>

<p align="center">
  连接你的设备，在浏览器中管理 WireGuard 网络、分组权限和服务转发。
</p>

<p align="center">
  <a href="https://github.com/touken928/WireHub/actions/workflows/integration.yml"><img src="https://img.shields.io/github/actions/workflow/status/touken928/WireHub/integration.yml?branch=main&style=for-the-badge&label=CI" alt="CI"></a>
  <a href="https://github.com/touken928/WireHub/pkgs/container/wirehub"><img src="https://img.shields.io/badge/Docker-GHCR-2496ED?style=for-the-badge&logo=docker&logoColor=white" alt="Docker image"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=for-the-badge" alt="GPL-3.0"></a>
</p>

<p align="center">
  <img src="docs/assets/overview.png" alt="WireHub 管理界面" width="960">
</p>

## 功能

- 查看设备连接状态与流量。
- 创建设备并下载 WireGuard 配置。
- 拖拽连接分组，设置单向、双向和组内访问权限。
- 将设备服务通过统一地址转发给指定分组。

## 快速开始

将管理口令替换为自己的随机口令，然后启动：

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

打开 **[http://localhost:51820](http://localhost:51820)**，输入同一管理口令并点击 **Connect**。

远程服务器可先运行 `ssh -L 51820:127.0.0.1:51820 user@server`，再在本地浏览器打开上述地址。管理端口默认仅在服务器本机开放；需要公网管理时，请配置 HTTPS 反向代理。

### 首次设置

| 字段 | 填写方式 |
| --- | --- |
| **Subnet** | 默认 `10.10.10.0/24`，选择与现有网络不冲突的网段。 |
| **Endpoint** | 服务器公网 IP 或域名加端口，例如 `vpn.example.com:51820`。 |
| **Keepalive** | 默认 `25` 秒。 |

点击 **Create network** 完成设置。网段创建后不可更改；默认 Hub 地址为 `10.10.10.1`，设备从 `10.10.10.2` 开始分配。服务器需允许 UDP `51820`。

## 使用

### 添加设备

1. 在 **Groups** 中点击 **New group** 创建分组。
2. 在 **Peers** 中点击 **New peer**，填写名称并选择分组。
3. 下载生成的 `.conf`，导入 [WireGuard 客户端](https://www.wireguard.com/install/)并连接。

配置仅在创建设备时提供，请保存好。需要重新配置时，删除设备并重新创建。

### 设置访问权限

在 **Groups** 中，从一个分组的连接点拖到另一个分组：

- **One way**：只允许起点分组访问终点分组。
- **Both ways**：允许两个分组相互访问。
- **Intra-group access**：在分组详情中启用，允许同组设备互访。

点击 **Save** 应用更改；选中连接后按 **Delete** 可移除权限。未授权的设备间访问默认禁止。

### 转发服务

在 **Forwards** 中点击 **New forward**，选择目标设备、TCP/UDP、服务端口和允许使用的分组。来源分组还需在 **Groups** 中拥有到目标分组的访问权限。

例如将某台设备的 TCP `8080` 服务转发后，授权设备通过 **`10.10.10.1:8080`** 访问。

### 调整设置与更新

在 **Settings** 中修改 Endpoint 和 Keepalive。修改只影响之后生成的设备配置。

更新时拉取镜像，停止并删除旧容器，再使用同一个 `wirehub-data` 数据卷运行上述启动命令。数据卷保存网络配置，请保留并备份。

---

镜像支持 **amd64 / arm64**，每次推送版本 tag 自动发布。历史版本见 [`v0`](https://github.com/touken928/WireHub/tree/v0)。
