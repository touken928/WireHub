package repo

import (
	"github.com/touken928/wirehub/internal/config"
)

// UpstreamDNSOrDefault returns configured upstream resolvers, or the product defaults.
func (s *Settings) UpstreamDNSOrDefault() []string {
	if len(s.UpstreamDNS) == 0 {
		return append([]string(nil), config.DefaultUpstreamDNS...)
	}
	return append([]string(nil), s.UpstreamDNS...)
}

// ClientDNS returns hub DNS followed by upstream resolvers for WireGuard client configs.
func (s *Settings) ClientDNS() []string {
	out := make([]string, 0, 1+len(s.UpstreamDNSOrDefault()))
	out = append(out, s.DNSIP)
	out = append(out, s.UpstreamDNSOrDefault()...)
	return out
}
