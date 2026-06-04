package config

import (
	"fmt"
	"net/netip"
	"strings"
)

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
		return nil, nil
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
