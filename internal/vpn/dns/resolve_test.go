package dns

import (
	"net/netip"
	"strings"
	"testing"

	"github.com/touken928/wirehub/internal/domain/runtime"
)

func TestResolveHostExternalViaUpstream(t *testing.T) {
	s := NewServer("100.127.0.1", []string{"114.114.114.114"})
	s.UpdateDNS(runtime.DNSCatalog{HubIP: "100.127.0.1"}, nil)
	addr, err := s.ResolveHost("example.com")
	if err != nil {
		t.Skipf("upstream dns unavailable: %v", err)
	}
	if !addr.Is4() {
		t.Fatalf("expected IPv4, got %s", addr)
	}
}

func TestResolveHostExternalWithoutUpstream(t *testing.T) {
	s := NewServer("100.127.0.1", nil)
	_, err := s.ResolveHost("example.com")
	if err == nil {
		t.Fatal("expected error without upstream DNS")
	}
	if !strings.Contains(err.Error(), "upstream") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestResolveHostUnreachableUpstream(t *testing.T) {
	s := NewServer("100.127.0.1", []string{"203.0.113.53"})
	_, err := s.ResolveHost("example.com")
	if err == nil {
		t.Skip("upstream unexpectedly reachable in this environment")
	}
	if !strings.Contains(err.Error(), "upstream") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestResolveHostExternalViaUpstreamNotVPNLoop(t *testing.T) {
	s := NewServer("100.127.0.1", []string{"100.127.0.1", "114.114.114.114"})
	addr, err := s.ResolveHost("example.com")
	if err != nil {
		t.Skipf("upstream dns unavailable: %v", err)
	}
	if addr == (netip.Addr{}) {
		t.Fatal("expected address")
	}
}
