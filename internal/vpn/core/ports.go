// Package core holds VPN data-plane constants shared across layers (no netstack imports).
package core

const (
	// HubDNSPort is authoritative DNS on the hub VPN address.
	HubDNSPort = 53
	// HubTunnelWebPort is admin UI/API on the hub VPN address inside the tunnel.
	HubTunnelWebPort = 80
)
