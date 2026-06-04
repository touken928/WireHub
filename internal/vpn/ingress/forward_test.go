package ingress

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/netip"
	"testing"
	"time"
)

func TestForwardProxyTCPRelay(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	backend := netip.MustParseAddr("100.127.0.2")
	const (
		listenPort  = 19001
		backendPort = 19002
	)

	tnet, cleanup := newTestNetstack(t, hub, backend)
	defer cleanup()

	ln, err := tnet.ListenTCPAddrPort(netip.AddrPortFrom(backend, backendPort))
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	go http.Serve(ln, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "forward-target-ok")
	}))

	proxy, err := NewForwardProxy(tnet, hub.String(), "100.127.0.0/24", staticHostResolver{
		hosts: map[string]netip.Addr{backend.String(): backend},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer proxy.Stop()

	if err := proxy.Apply([]ForwardRule{{
		ListenPort: listenPort,
		Protocol:   "tcp",
		TargetHost: backend.String(),
		TargetPort: backendPort,
	}}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(50 * time.Millisecond)

	url := fmt.Sprintf("http://%s/", netip.AddrPortFrom(hub, listenPort))
	resp, err := testHTTPClient(tnet).Get(url)
	if err != nil {
		t.Fatalf("GET hub forward listen port: %v", err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatal(err)
	}
	if string(body) != "forward-target-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestForwardProxyRejectsUnsupportedProtocol(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	tnet, cleanup := newTestNetstack(t, hub)
	defer cleanup()

	proxy, err := NewForwardProxy(tnet, hub.String(), "100.127.0.0/24", staticHostResolver{hosts: nil})
	if err != nil {
		t.Fatal(err)
	}
	defer proxy.Stop()

	// Apply returns nil; unsupported protocol fails inside the rule goroutine.
	if err := proxy.Apply([]ForwardRule{{
		ListenPort: 19004,
		Protocol:   "sctp",
		TargetHost: "100.127.0.2",
		TargetPort: 80,
	}}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(20 * time.Millisecond)

	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()
	if _, err := tnet.DialContext(ctx, "tcp", netip.AddrPortFrom(hub, 19004).String()); err == nil {
		t.Fatal("unsupported protocol must not open listener")
	}
}
