// Package l4 is hub Layer-4 relay on the VPN netstack address.
//
// Three mechanisms share packet/port helpers but differ in how clients connect:
//
//   - System listen: built-in services on hub IP — authoritative DNS (:53 UDP),
//     tunnel Web/API (CLI --port, default 8443 TCP), WireGuard (same --port UDP).
//     Managed by vpn.Stack / vpn/dns, not the Forward admin page.
//
//   - Forward listen: admin Forward rules — client dials hub:listenPort;
//     ForwardProxy accepts and dials targetHost:targetPort.
//
//   - Transparent relay: unidirectional group links — client dials target peer
//     WG IP:servicePort unchanged; TransparentTable SNATs on the hub TUN path
//     (ephemeral hub source ports, return DNAT on TUN Write).
//
// TransparentTable.ReserveHubPorts must include system + forward listen ports so
// ephemeral SNAT does not collide with them.
package l4

const (
	// HubDNSPort is the authoritative DNS listen port on the hub VPN address.
	HubDNSPort = 53
)
