package dns

import (
	"fmt"
	"net"
	"net/netip"
	"strings"

	"github.com/miekg/dns"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain"
)

// ResolveHost resolves a forward target to an IPv4 address using hub DNS and upstream resolvers.
func (s *Server) ResolveHost(host string) (netip.Addr, error) {
	host, err := domain.ValidateForwardTargetHost(host)
	if err != nil {
		return netip.Addr{}, err
	}

	if ip := net.ParseIP(host); ip != nil {
		addr, ok := netip.AddrFromSlice(ip.To4())
		if !ok {
			return netip.Addr{}, fmt.Errorf("invalid target ip")
		}
		return addr, nil
	}

	name := strings.TrimSuffix(strings.ToLower(host), ".")
	if s.isInternalName(name) {
		if !strings.HasSuffix(name, "."+config.DNSDomain) {
			return netip.Addr{}, fmt.Errorf("unknown host %q", host)
		}
		slug := strings.TrimSuffix(name, "."+config.DNSDomain)
		if slug == "" {
			return netip.Addr{}, fmt.Errorf("unknown host %q", host)
		}
		ip, found := s.lookupIP(slug, true)
		if !found {
			return netip.Addr{}, fmt.Errorf("unknown host %q", host)
		}
		addr, err := netip.ParseAddr(ip)
		if err != nil {
			return netip.Addr{}, fmt.Errorf("resolve %q: %w", host, err)
		}
		return addr, nil
	}

	return s.resolveUpstreamA(name)
}

func (s *Server) resolveUpstreamA(name string) (netip.Addr, error) {
	if len(s.upstream) == 0 {
		return netip.Addr{}, fmt.Errorf("no upstream DNS configured for %q", name)
	}
	msg := new(dns.Msg)
	msg.SetQuestion(dns.Fqdn(name), dns.TypeA)
	resp, err := s.exchangeUpstream(msg)
	if err != nil {
		return netip.Addr{}, fmt.Errorf("resolve %q: %w", name, err)
	}
	for _, ans := range resp.Answer {
		if a, ok := ans.(*dns.A); ok && a.A != nil {
			addr, ok := netip.AddrFromSlice(a.A)
			if ok {
				return addr, nil
			}
		}
	}
	return netip.Addr{}, fmt.Errorf("no A record for %q", name)
}
