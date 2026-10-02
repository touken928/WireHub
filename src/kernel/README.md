# Network kernel

All data-plane code lives in this directory. `mod.rs` exposes the UDP router,
readiness, reload commands and runtime statistics to the application. Network
configuration validation is available through `config`.

| Module | Responsibility |
| --- | --- |
| `ipv4.rs` | Strict IPv4 envelope validation, authenticated source validation and one ingress TTL decrement |
| `protocol/` | TCP, UDP and ICMP implementations of the same parsing, association and rewriting contract |
| `checksum.rs` | IPv4/ICMP checksums, transport pseudo-header checksums and incremental correction of quoted packets |
| `flows/` | Shared direct/forward allocation, reverse indices, delivery reservations, quotas, expiry and revocation |
| `policy.rs` | Directed group ACLs, forward authorization and assigned source-address checks |
| `router/` | WireGuard authentication, runtime snapshots, packet routing, bounded pending delivery and statistics |
| `config.rs` | Subnet, endpoint and keepalive validation |

## Protocol contract

`PacketProtocol` defines `parse`, `association` and `rewrite`. `TransportPacket`
dispatches to the TCP, UDP or ICMP implementation using a closed enum and static
dispatch. IPv4 validation precedes transport parsing, and invalid packets are
rejected before routing.

The router consumes three association types:

- `Connection`: TCP/UDP packets use the shared reverse lookup and direct/forward
  flow allocation path. A new TCP flow requires an initial SYN without ACK.
- `Related`: ICMP errors may only use a committed live TCP/UDP mapping from the
  authenticated backend. A lookup miss is dropped; it cannot fall back to an ACL
  route, allocate a flow or refresh its idle timer.
- `Stateless`: ICMP Echo and other IPv4 protocols follow directed ACL routing
  without creating flow state.

`RewritePlan` distinguishes unchanged packets, ordinary address/port translation
and related-error translation. Each protocol rejects incompatible rewrite plans.
Protocol code updates transport contents and checksums; the IPv4 envelope then
updates addresses and its checksum without decrementing TTL again. ICMP errors
also restore quoted addresses/ports and correct available quoted checksums,
including truncated quotes and IPv4 UDP packets with checksum disabled.

## Flow lifecycle

`Flows` owns indices and resource limits. A reservation is exclusive while new,
and it must be completed with the actual delivery result. Only successful
delivery commits state or refreshes idle time. Generation checks prevent expired
or revoked reservations from restoring removed mappings. The pending WireGuard
delivery queue retains source, destination and route provenance for revalidation.

`FlowState::on_delivered` and `deadline` delegate lifecycle behavior to protocol
implementations. TCP owns handshake tracking, FIN direction tracking and the
fixed RST/bidirectional-FIN grace period. UDP owns its idle timeout. Established
TCP expires after 2 hours 4 minutes; incomplete TCP handshakes and UDP expire
after 60 seconds; closing TCP uses a fixed 30-second grace. Expiry silently
reclaims state. Limits remain 256 active plus pending flows per initiating peer
and 16,384 globally, across TCP and UDP.

Snapshot reconciliation supplies borrowed `PeerPolicy` views, so flow management
does not depend on WireGuard tunnel objects. Acknowledged ACL revocation removes
affected flows immediately. Authorized flows and authenticated tunnels survive
unrelated reloads; failed snapshots clear routing state and trigger retries.

Tests live beside their respective modules. HTTP management and persistence stay
in `src/api` and `src/storage`; their public API and database schema are unchanged.
