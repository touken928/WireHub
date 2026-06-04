package dns

import (
	"strings"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
)

// dnsSlug strips a leading www. prefix for alias resolution.
// www.{name}.wirehub -> {name}; www.hub.wirehub -> hub.
func dnsSlug(slug string) string {
	slug = strings.ToLower(strings.TrimSpace(slug))
	if strings.HasPrefix(slug, "www.") {
		return strings.TrimPrefix(slug, "www.")
	}
	return slug
}

func (s *Server) lookupIP(raw string, ok bool) (string, bool) {
	return s.lookupIPForClient(raw, ok, "")
}

func (s *Server) lookupIPForClient(raw string, ok bool, clientIP string) (string, bool) {
	if !ok {
		return "", false
	}
	raw = strings.ToLower(strings.TrimSpace(raw))

	slug := dnsSlug(raw)
	if slug == config.HubDNSLabel {
		return s.hubIP, true
	}
	if ip, found := s.store.LookupMapVIP(slug); found {
		if clientIP != "" {
			svcMap, err := s.store.GetServiceMapBySlug(slug)
			if err != nil {
				return "", false
			}
			allowed, err := s.store.MapAllowedForPeer(clientIP, svcMap.ID)
			if err != nil || !allowed {
				return "", false
			}
		}
		return ip, true
	}
	return s.lookupPeer(slug)
}

func (s *Server) lookupPeer(slug string) (string, bool) {
	if ip, found := s.store.ResolveDNS(slug); found {
		return ip, true
	}
	var peers []repo.Peer
	if err := s.store.DB().Where("dns_name = ?", slug).Find(&peers).Error; err == nil && len(peers) > 0 {
		return peers[0].WGIP, true
	}
	return "", false
}
