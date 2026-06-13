package runtime

import (
	"net/netip"
	"testing"

	"github.com/touken928/wirehub/internal/config"
	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/ingress"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// mockCallbacks returns a fixed SyncBundle.
type mockCallbacks struct {
	bundle domainruntime.SyncBundle
}

func (m *mockCallbacks) LoadSyncBundle() (domainruntime.SyncBundle, error) {
	return m.bundle, nil
}

func (m *mockCallbacks) OnStarted(_ Dataplane) {}
func (m *mockCallbacks) OnStopped()             {}

// mockResolver is a HostResolver that always resolves to a fixed address.
type mockResolver struct {
	addr netip.Addr
}

func (r mockResolver) ResolveHost(_ string) (netip.Addr, error) {
	return r.addr, nil
}

func (r mockResolver) ResolveForwardAddrs(_ string) ([]netip.Addr, error) {
	return []netip.Addr{r.addr}, nil
}

func TestStackSyncPortForwardsPropagatesApplyError(t *testing.T) {
	const hubIP = "100.127.0.1"
	const dnsIP = "100.127.0.2"
	const conflictPort = 20001

	hubAddr := netip.MustParseAddr(hubIP)
	dnsAddr := netip.MustParseAddr(dnsIP)

	// Create a real tunnel manager (uses userspace WireGuard — no root required).
	mgr, err := tunnel.NewManager(hubIP, dnsIP, nil, 51820, 1420)
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	// Pre-bind a port on the netstack to cause a forward-apply conflict.
	preLn, err := mgr.Net().ListenTCPAddrPort(netip.AddrPortFrom(hubAddr, conflictPort))
	if err != nil {
		t.Fatal(err)
	}
	defer preLn.Close()

	// Create a ForwardProxy directly (same pattern as stack's applyIngressLocked).
	fp, err := ingress.NewForwardProxy(mgr.Net(), hubIP, "100.127.0.0/24", mockResolver{addr: dnsAddr})
	if err != nil {
		t.Fatal(err)
	}
	defer fp.Stop()

	cb := &mockCallbacks{
		bundle: domainruntime.SyncBundle{
			Settings: domainruntime.NetworkSettings{HubIP: hubIP},
			Forwards: []domainruntime.ForwardRule{
				{ListenPort: conflictPort, Protocol: "tcp", TargetHost: "10.0.0.1", TargetPort: 80},
			},
		},
	}

	// Build a Stack with the real tunnel manager and forward proxy.
	s := &Stack{
		cfg:          &config.RuntimeConfig{},
		cb:           cb,
		tunnelMgr:    mgr,
		forwardProxy: fp,
	}

	// SyncPortForwards must return an error because the port is already bound.
	err = s.SyncPortForwards()
	if err == nil {
		t.Fatal("expected SyncPortForwards to return error when port is already bound")
	}
	t.Logf("SyncPortForwards error: %v", err)
}
