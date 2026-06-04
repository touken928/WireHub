package runtime

import (
	dompolicy "github.com/touken928/wirehub/internal/domain/policy"
	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// Dataplane is the VPN data-plane API consumed by the control plane.
type Dataplane interface {
	Start(bundle runtime.SyncBundle) error
	Stop() error
	ReloadSettings() error
	SyncPortForwards() error
	SyncMaps() error
	HubListenPort() int
	SyncPeer(peer runtime.WGPeer) error
	RemovePeer(publicKey string) error
	ApplyPolicy(spec dompolicy.AccessPolicySpec) error
	UpdateDNS(catalog runtime.DNSCatalog, peers []runtime.WGPeer) error
	FullSync(bundle runtime.SyncBundle) error
	GetStats() (map[string]tunnel.PeerStats, error)
}
