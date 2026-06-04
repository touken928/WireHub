package ingress

import (
	"net/http"
	"net/netip"
	"testing"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

func newTestNetstack(t *testing.T, addrs ...netip.Addr) (*netstack.Net, func()) {
	t.Helper()
	if len(addrs) == 0 {
		t.Fatal("at least one address required")
	}
	tun, tnet, err := netstack.CreateNetTUN(addrs, []netip.Addr{addrs[0]}, 1420)
	if err != nil {
		t.Fatal(err)
	}
	return tnet, func() { _ = tun.Close() }
}

func testHTTPClient(tnet *netstack.Net) *http.Client {
	return &http.Client{
		Transport: &http.Transport{DialContext: tnet.DialContext},
		Timeout:   3 * time.Second,
	}
}

type staticHostResolver struct {
	hosts map[string]netip.Addr
}

func (r staticHostResolver) ResolveHost(host string) (netip.Addr, error) {
	if addr, ok := r.hosts[host]; ok {
		return addr, nil
	}
	return netip.Addr{}, errUnknownHost{host: host}
}

func (r staticHostResolver) ResolveForwardAddrs(host string) ([]netip.Addr, error) {
	addr, err := r.ResolveHost(host)
	if err != nil {
		return nil, err
	}
	return []netip.Addr{addr}, nil
}

type errUnknownHost struct{ host string }

func (e errUnknownHost) Error() string {
	return "unknown host: " + e.host
}
