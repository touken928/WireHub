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

func TestForwardProxyApplyRejectsUnsupportedProtocolSynchronously(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	tnet, cleanup := newTestNetstack(t, hub)
	defer cleanup()

	proxy, err := NewForwardProxy(tnet, hub.String(), "100.127.0.0/24", staticHostResolver{hosts: nil})
	if err != nil {
		t.Fatal(err)
	}
	defer proxy.Stop()

	// Apply must return an error synchronously for unsupported protocol.
	err = proxy.Apply([]ForwardRule{{
		ListenPort: 19004,
		Protocol:   "sctp",
		TargetHost: "100.127.0.2",
		TargetPort: 80,
	}})
	if err == nil {
		t.Fatal("expected Apply to return error for unsupported protocol")
	}

	// Ensure no listener was opened on the port.
	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()
	if _, err := tnet.DialContext(ctx, "tcp", netip.AddrPortFrom(hub, 19004).String()); err == nil {
		t.Fatal("unsupported protocol must not open listener")
	}
}

func TestForwardProxyApplyReturnsBindError(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	tnet, cleanup := newTestNetstack(t, hub)
	defer cleanup()

	const bindPort = 19005

	// Pre-bind the port to cause a conflict.
	preLn, err := tnet.ListenTCPAddrPort(netip.AddrPortFrom(hub, bindPort))
	if err != nil {
		t.Fatal(err)
	}
	defer preLn.Close()

	proxy, err := NewForwardProxy(tnet, hub.String(), "100.127.0.0/24", staticHostResolver{hosts: nil})
	if err != nil {
		t.Fatal(err)
	}
	defer proxy.Stop()

	// Apply must fail because the port is already bound.
	err = proxy.Apply([]ForwardRule{{
		ListenPort: bindPort,
		Protocol:   "tcp",
		TargetHost: "100.127.0.2",
		TargetPort: 80,
	}})
	if err == nil {
		t.Fatal("expected Apply to return error when port is already bound")
	}
	t.Logf("bind error: %v", err)
}

func TestForwardProxyApplyRejectsDuplicatePort(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	tnet, cleanup := newTestNetstack(t, hub)
	defer cleanup()

	proxy, err := NewForwardProxy(tnet, hub.String(), "100.127.0.0/24", staticHostResolver{hosts: nil})
	if err != nil {
		t.Fatal(err)
	}
	defer proxy.Stop()

	// Two rules with the same listen port must be rejected.
	err = proxy.Apply([]ForwardRule{
		{ListenPort: 19006, Protocol: "tcp", TargetHost: "10.0.0.1", TargetPort: 80},
		{ListenPort: 19006, Protocol: "udp", TargetHost: "10.0.0.1", TargetPort: 53},
	})
	if err == nil {
		t.Fatal("expected Apply to return error for duplicate listen port")
	}
	t.Logf("duplicate port error: %v", err)
}

func TestForwardProxyApplyRollbackOnBindFailure(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	tnet, cleanup := newTestNetstack(t, hub)
	defer cleanup()

	const (
		goodPort = 19007
		badPort  = 19008
	)

	// Pre-bind badPort so the second rule fails.
	preLn, err := tnet.ListenTCPAddrPort(netip.AddrPortFrom(hub, badPort))
	if err != nil {
		t.Fatal(err)
	}
	defer preLn.Close()

	proxy, err := NewForwardProxy(tnet, hub.String(), "100.127.0.0/24", staticHostResolver{hosts: nil})
	if err != nil {
		t.Fatal(err)
	}
	defer proxy.Stop()

	// Apply with one good rule and one that will fail to bind.
	err = proxy.Apply([]ForwardRule{
		{ListenPort: goodPort, Protocol: "tcp", TargetHost: "10.0.0.1", TargetPort: 80},
		{ListenPort: badPort, Protocol: "tcp", TargetHost: "10.0.0.2", TargetPort: 443},
	})
	if err == nil {
		t.Fatal("expected Apply to return error")
	}
	t.Logf("rollback error: %v", err)

	// The good port must NOT be bound after rollback (was pre-bound then closed).
	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()
	if _, err := tnet.DialContext(ctx, "tcp", netip.AddrPortFrom(hub, goodPort).String()); err == nil {
		t.Fatal("good port must not be listening after rollback")
	}
}
