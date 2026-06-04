package integration

import (
	"net/netip"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/domain/policy"
	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
	"github.com/touken928/wirehub/internal/vpn/ingress"
	vpnnetstack "github.com/touken928/wirehub/internal/vpn/netstack"
	vpnpolicy "github.com/touken928/wirehub/internal/vpn/policy"
	"github.com/touken928/wirehub/internal/vpn/tun"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

func applyAccessFromStore(st *repo.Store, wgMgr *tunnel.Manager) error {
	pol, err := buildAccessPolicyFromStore(st)
	if err != nil {
		return err
	}
	wgMgr.SetAccessPolicy(pol)
	return nil
}

func buildAccessPolicyFromStore(st *repo.Store) (*tun.AccessPolicy, error) {
	peers, err := st.ListPeers()
	if err != nil {
		return nil, err
	}
	links, err := st.ListGroupLinks()
	if err != nil {
		return nil, err
	}
	eps := make([]policy.PeerEndpoint, len(peers))
	for i, p := range peers {
		eps[i] = policy.PeerEndpoint{
			ID: p.ID, Name: p.Name, DNSName: p.DNSName,
			WGIP: p.WGIP, GroupID: p.GroupID, Enabled: p.Enabled,
		}
	}
	pairs := make([]policy.GroupLinkPair, len(links))
	for i, l := range links {
		pairs[i] = policy.GroupLinkPair{
			FromGroupID: l.FromGroupID, ToGroupID: l.ToGroupID, Bidirectional: l.Bidirectional,
		}
	}
	groups, err := st.ListGroups()
	if err != nil {
		return nil, err
	}
	groupAccess := make([]policy.GroupAccess, len(groups))
	for i, g := range groups {
		groupAccess[i] = policy.GroupAccess{ID: g.ID, AllowIntraGroup: g.AllowIntraGroup}
	}
	maps, err := st.ListMapDetails()
	if err != nil {
		return nil, err
	}
	mapAccess := make([]policy.MapAccess, 0, len(maps))
	for _, r := range maps {
		mapAccess = append(mapAccess, policy.NewMapAccess(r.VirtualIP, r.AllowedGroups))
	}
	spec, err := policy.BuildAccessPolicySpec(eps, pairs, policy.NewGroupAccessPolicy(groupAccess), policy.NewMapAccessPolicy(mapAccess))
	if err != nil {
		return nil, err
	}
	return vpnpolicy.Apply(spec), nil
}

func buildSyncBundleFromStore(st *repo.Store) (runtime.SyncBundle, error) {
	return service.NewApp(st).LoadSyncBundle()
}

func forwardRulesFromRepo(rules []repo.PortForward) []ingress.ForwardRule {
	out := make([]ingress.ForwardRule, 0, len(rules))
	for _, r := range rules {
		out = append(out, ingress.ForwardRule{
			ID:         r.ID,
			ListenPort: r.ListenPort,
			Protocol:   r.Protocol,
			TargetHost: r.TargetHost,
			TargetPort: r.TargetPort,
		})
	}
	return out
}

func mapRulesFromBundle(maps []runtime.MapRule) []ingress.MapRule {
	out := make([]ingress.MapRule, 0, len(maps))
	for _, r := range maps {
		vip, err := netip.ParseAddr(r.VirtualIP)
		if err != nil {
			continue
		}
		out = append(out, ingress.MapRule{
			ID:              r.ID,
			Slug:            r.Slug,
			TargetHost:      r.TargetHost,
			VirtualIP:       vip,
			AllowedGroupIDs: r.AllowedGroupIDs,
		})
	}
	return out
}

func mapVIPsFromBundle(maps []runtime.MapRule) []netip.Addr {
	out := make([]netip.Addr, 0, len(maps))
	for _, r := range maps {
		vip, err := netip.ParseAddr(r.VirtualIP)
		if err == nil && vip.IsValid() {
			out = append(out, vip)
		}
	}
	return out
}

func (env *meshEnv) applyPortForwards(t *testing.T) {
	t.Helper()
	settings, err := env.store.GetSettings()
	if err != nil {
		t.Fatal(err)
	}
	if env.forwardProxy == nil {
		mgr, err := ingress.NewForwardProxy(env.wgMgr.Net(), env.hubIP, settings.WGSubnet, env.dnsServer)
		if err != nil {
			t.Fatal(err)
		}
		env.forwardProxy = mgr
	}
	rules, err := env.store.ListPortForwards()
	if err != nil {
		t.Fatal(err)
	}
	runtimeRules := forwardRulesFromRepo(rules)
	env.wgMgr.ReserveHubPorts(ingress.ReservedHubPorts(ingress.HubTunnelWebPort, runtimeRules))
	if err := env.forwardProxy.Apply(runtimeRules); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)
}

func (env *meshEnv) restartHubTunnelWithMaps(t *testing.T, bundle runtime.SyncBundle) {
	t.Helper()
	settings, err := env.store.GetSettings()
	if err != nil {
		t.Fatal(err)
	}
	if env.mapProxy != nil {
		env.mapProxy.Stop()
		env.mapProxy = nil
	}
	if env.forwardProxy != nil {
		env.forwardProxy.Stop()
		env.forwardProxy = nil
	}
	_ = env.dnsServer.Stop()
	_ = env.wgMgr.Down()
	_ = env.wgMgr.Close()

	wgMgr, err := tunnel.NewManager(settings.HubIP, settings.DNSIP, mapVIPsFromBundle(bundle.Maps), settings.ListenPort, settings.MTU)
	if err != nil {
		t.Fatal(err)
	}
	if err := wgMgr.ConfigureServer(settings.ServerPrivateKey, settings.ListenPort); err != nil {
		t.Fatal(err)
	}
	if err := wgMgr.Up(); err != nil {
		t.Fatal(err)
	}
	for i := range env.peers {
		p := &env.peers[i].Peer
		if err := wgMgr.SyncPeer(runtime.WGPeer{
			ID: p.ID, PublicKey: p.PublicKey, WGIP: p.WGIP,
			DNSName: p.DNSName, GroupID: p.GroupID, Enabled: p.Enabled,
		}); err != nil {
			t.Fatal(err)
		}
	}
	if err := vpnnetstack.EnableForwarding(wgMgr.Net()); err != nil {
		t.Fatal(err)
	}
	if err := applyAccessFromStore(env.store, wgMgr); err != nil {
		t.Fatal(err)
	}
	if err := env.dnsServer.StartOnNetstack(wgMgr.Net(), settings.DNSIP, 53); err != nil {
		t.Fatal(err)
	}
	env.dnsServer.UpdateDNS(bundle.DNS, bundle.Peers)
	env.wgMgr = wgMgr
	env.reconnectAll(t)
}

func (env *meshEnv) reconnectAll(t *testing.T) {
	t.Helper()
	for i := range env.peers {
		if env.peers[i].Dev != nil {
			env.peers[i].Dev.Close()
			env.peers[i].Dev = nil
			env.peers[i].Net = nil
		}
	}
	env.connectAll(t)
}

func (env *meshEnv) applyMaps(t *testing.T) {
	t.Helper()
	bundle, err := buildSyncBundleFromStore(env.store)
	if err != nil {
		t.Fatal(err)
	}
	if len(bundle.Maps) > 0 {
		env.restartHubTunnelWithMaps(t, bundle)
	}
	settings, err := env.store.GetSettings()
	if err != nil {
		t.Fatal(err)
	}
	if env.mapProxy == nil {
		mgr, err := ingress.NewMapProxy(env.wgMgr.Net(), settings.WGSubnet, env.dnsServer)
		if err != nil {
			t.Fatal(err)
		}
		env.mapProxy = mgr
	}
	rules := mapRulesFromBundle(bundle.Maps)
	if err := env.mapProxy.Apply(rules, bundle.Peers); err != nil {
		t.Fatal(err)
	}
	env.wgMgr.SetMapVIPs(mapVIPsFromBundle(bundle.Maps))
	env.dnsServer.UpdateDNS(bundle.DNS, bundle.Peers)
	if err := applyAccessFromStore(env.store, env.wgMgr); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)
}

// applyMapsRuntimeSync mirrors production Stack.FullSync map apply (no hub tunnel recreation).
func (env *meshEnv) applyMapsRuntimeSync(t *testing.T, registerVIPs bool) {
	t.Helper()
	bundle, err := buildSyncBundleFromStore(env.store)
	if err != nil {
		t.Fatal(err)
	}
	settings, err := env.store.GetSettings()
	if err != nil {
		t.Fatal(err)
	}
	if registerVIPs && len(bundle.Maps) > 0 {
		vips := mapVIPsFromBundle(bundle.Maps)
		if err := env.wgMgr.EnsureMapVIPs(vips); err != nil {
			t.Fatal(err)
		}
		env.wgMgr.SetMapVIPs(vips)
	}
	if env.mapProxy == nil {
		mgr, err := ingress.NewMapProxy(env.wgMgr.Net(), settings.WGSubnet, env.dnsServer)
		if err != nil {
			t.Fatal(err)
		}
		env.mapProxy = mgr
	}
	rules := mapRulesFromBundle(bundle.Maps)
	if err := env.mapProxy.Apply(rules, bundle.Peers); err != nil {
		t.Fatal(err)
	}
	env.wgMgr.SetMapVIPs(mapVIPsFromBundle(bundle.Maps))
	env.dnsServer.UpdateDNS(bundle.DNS, bundle.Peers)
	if err := applyAccessFromStore(env.store, env.wgMgr); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)
}
