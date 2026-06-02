package dns

import (
	"strings"

	"github.com/touken928/wirehub/internal/store"
)

// dnsSlug strips a leading www. prefix for alias resolution.
// www.wirehub.internal -> hub (""); www.{name}.wirehub.internal -> {name}.
func dnsSlug(slug string) string {
	slug = strings.ToLower(strings.TrimSpace(slug))
	if slug == "www" {
		return ""
	}
	if strings.HasPrefix(slug, "www.") {
		return strings.TrimPrefix(slug, "www.")
	}
	return slug
}

func (s *Server) lookupIP(raw string, ok bool) (string, bool) {
	if !ok {
		return "", false
	}
	raw = strings.ToLower(strings.TrimSpace(raw))

	slug := dnsSlug(raw)
	if slug == "" {
		return s.hubIP, true
	}
	return s.lookupPeer(slug)
}

func (s *Server) lookupPeer(slug string) (string, bool) {
	if ip, found := s.store.ResolveDNS(slug); found {
		return ip, true
	}
	var peers []store.Peer
	if err := s.store.DB().Where("dns_name = ?", slug).Find(&peers).Error; err == nil && len(peers) > 0 {
		return peers[0].WGIP, true
	}
	return "", false
}
