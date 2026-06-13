package service

import (
	dompolicy "github.com/touken928/wirehub/internal/domain/policy"
	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// NetworkRuntime controls VPN stack lifecycle.
type NetworkRuntime interface {
	Start(bundle domainruntime.SyncBundle) error
	Stop() error
	ReloadSettings() error
	SyncPortForwards() error
	SyncMaps() error
	HubListenPort() int
	SetDNSUpstream(upstream []string)
}

// Dataplane is the live VPN data plane as consumed by service.
// Keep this interface service-owned so control-plane code does not depend on
// concrete vpn/runtime types outside the lifecycle bridge.
type Dataplane interface {
	Start(bundle domainruntime.SyncBundle) error
	Stop() error
	ReloadSettings() error
	SyncPortForwards() error
	SyncMaps() error
	HubListenPort() int
	SyncPeer(peer domainruntime.WGPeer) error
	RemovePeer(publicKey string) error
	ApplyPolicy(spec dompolicy.AccessPolicySpec) error
	UpdateDNS(catalog domainruntime.DNSCatalog, peers []domainruntime.WGPeer) error
	FullSync(bundle domainruntime.SyncBundle) error
	GetStats() (map[string]tunnel.PeerStats, error)
}
