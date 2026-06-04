package repo

// UpstreamDNSResolvers returns configured upstream resolvers (may be empty).
func (s *Settings) UpstreamDNSResolvers() []string {
	if len(s.UpstreamDNS) == 0 {
		return nil
	}
	return append([]string(nil), s.UpstreamDNS...)
}

// ClientDNS returns only the hub DNS IP for WireGuard client configs.
// Upstream resolvers are used server-side for external names; listing them on
// clients causes macOS to fall back to public DNS for *.wirehub and cache NXDOMAIN.
func (s *Settings) ClientDNS() []string {
	return []string{s.DNSIP}
}
