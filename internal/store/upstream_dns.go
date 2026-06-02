package store

import (
	"fmt"
	"net/netip"
	"strings"

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

// ParseUpstreamDNS validates resolver addresses (one per line or comma-separated).
func ParseUpstreamDNS(raw []string) ([]string, error) {
	var lines []string
	for _, part := range raw {
		for _, line := range strings.Split(part, "\n") {
			line = strings.TrimSpace(line)
			if line == "" {
				continue
			}
			for _, item := range strings.Split(line, ",") {
				item = strings.TrimSpace(item)
				if item != "" {
					lines = append(lines, item)
				}
			}
		}
	}
	if len(lines) == 0 {
		return append([]string(nil), config.DefaultUpstreamDNS...), nil
	}

	seen := make(map[string]struct{}, len(lines))
	out := make([]string, 0, len(lines))
	for _, line := range lines {
		addr, err := netip.ParseAddr(line)
		if err != nil {
			return nil, fmt.Errorf("invalid dns address %q", line)
		}
		key := addr.String()
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		out = append(out, key)
	}
	return out, nil
}
