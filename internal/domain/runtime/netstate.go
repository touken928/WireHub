package runtime

import (
	"github.com/touken928/wirehub/internal/domain/policy"
)

// NetworkSettings is hub VPN runtime configuration (portable, no GORM).
type NetworkSettings struct {
	HubIP            string
	DNSIP            string
	WGSubnet         string
	ServerPrivateKey string
	MTU              int
	ListenPort       int
	StatusInterval   int
	UpstreamDNS      []string
}

// WGPeer is the minimum peer data for WireGuard and DNS sync.
type WGPeer struct {
	ID        uint
	PublicKey string
	WGIP      string
	DNSName   string
	GroupID   uint
	Enabled   bool
}

// ForwardRule is a runtime port-forward rule.
type ForwardRule struct {
	ID         uint
	ListenPort int
	Protocol   string
	TargetHost string
	TargetPort int
}

// MapRule is a runtime service-map rule.
type MapRule struct {
	ID              uint
	Slug            string
	TargetHost      string
	VirtualIP       string
	AllowedGroupIDs map[uint]struct{}
}

// MapDNSEntry is authoritative DNS data for a map slug.
type MapDNSEntry struct {
	VirtualIP       string
	AllowedGroupIDs map[uint]struct{}
}

// DNSCatalog holds in-memory authoritative DNS state for the data plane.
type DNSCatalog struct {
	HubIP string
	Peers map[string]string // slug → WG IP
	Maps  map[string]MapDNSEntry
}

// SyncBundle is the full runtime snapshot pushed from control plane to data plane.
type SyncBundle struct {
	Settings NetworkSettings
	Peers    []WGPeer
	Policy   policy.AccessPolicySpec
	Forwards []ForwardRule
	Maps     []MapRule
	DNS      DNSCatalog
}

// MapVirtualIPs returns parsed VIP strings from map rules.
func (b SyncBundle) MapVirtualIPs() []string {
	out := make([]string, 0, len(b.Maps))
	for _, m := range b.Maps {
		if m.VirtualIP != "" {
			out = append(out, m.VirtualIP)
		}
	}
	return out
}

// BuildDNSCatalog builds a DNS catalog from settings, peers, and maps.
func BuildDNSCatalog(hubIP string, peers []WGPeer, maps []MapRule) DNSCatalog {
	c := DNSCatalog{
		HubIP: hubIP,
		Peers: make(map[string]string, len(peers)),
		Maps:  make(map[string]MapDNSEntry, len(maps)),
	}
	for _, p := range peers {
		if !p.Enabled {
			continue
		}
		slug := p.DNSName
		if slug == "" {
			slug = p.WGIP
		}
		if slug != "" && p.WGIP != "" {
			c.Peers[slug] = p.WGIP
		}
	}
	for _, m := range maps {
		if m.Slug == "" || m.VirtualIP == "" {
			continue
		}
		c.Maps[m.Slug] = MapDNSEntry{
			VirtualIP:       m.VirtualIP,
			AllowedGroupIDs: m.AllowedGroupIDs,
		}
	}
	return c
}
