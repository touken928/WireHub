package dns

import (
	"net/netip"
	"strings"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/policy"
)

// dnsSlug strips a leading www. prefix for alias resolution.
func dnsSlug(slug string) string {
	slug = strings.ToLower(strings.TrimSpace(slug))
	if strings.HasPrefix(slug, "www.") {
		return strings.TrimPrefix(slug, "www.")
	}
	return slug
}

func (s *Server) lookupIP(raw string, ok bool) (string, bool) {
	return s.lookupIPForClient(raw, ok, netip.Addr{})
}

func (s *Server) lookupIPForClient(raw string, ok bool, clientIP netip.Addr) (string, bool) {
	if !ok {
		return "", false
	}
	raw = strings.ToLower(strings.TrimSpace(raw))
	slug := dnsSlug(raw)
	state := s.catalog.snapshot()

	if slug == config.HubDNSLabel {
		if state.catalog.HubIP != "" {
			return state.catalog.HubIP, true
		}
		return "", false
	}
	if ent, found := state.catalog.Maps[slug]; found {
		if clientIP.IsValid() {
			gid, ok := state.peerGroup[clientIP]
			if !ok || !policy.GroupInAllowedSet(ent.AllowedGroupIDs, gid) {
				return "", false
			}
		}
		return ent.VirtualIP, true
	}
	if ip, found := state.catalog.Peers[slug]; found {
		return ip, true
	}
	return "", false
}
