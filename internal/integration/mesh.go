package integration

import (
	"fmt"
	"path/filepath"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/repo"
	dnssvc "github.com/touken928/wirehub/internal/vpn/dns"
	"github.com/touken928/wirehub/internal/vpn/ingress"
	vpnnetstack "github.com/touken928/wirehub/internal/vpn/netstack"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

// peerSpec describes a peer to create in the mesh store before WireGuard clients connect.
type peerSpec struct {
	Name      string
	GroupName string
}

// connectedPeer is a mesh peer after startPeerClient has attached Dev and Net.
type connectedPeer struct {
	Peer repo.Peer
	Dev  *device.Device
	Net  *netstack.Net
}

// meshEnv is a minimal in-process hub: SQLite store, tunnel manager, DNS, optional ingress.
type meshEnv struct {
	wgMgr        *tunnel.Manager
	dnsServer    *dnssvc.Server
	forwardProxy *ingress.ForwardProxy
	mapProxy     *ingress.MapProxy
	hubIP        string
	dnsIP        string
	hubPubKey    string
	listenPort   int
	store        *repo.Store
	peers        []connectedPeer
}

// setupMesh provisions a configured hub with the given peers and optional bidirectional group links
// (pair names refer to group names). Returns the hub netstack and a cleanup func.
func setupMesh(t *testing.T, specs []peerSpec, linkPairs [][2]string) (*meshEnv, *netstack.Net, func()) {
	t.Helper()
	if len(specs) == 0 {
		t.Fatal("at least one peer spec is required")
	}

	dir := t.TempDir()
	listenPort := freeUDPPort(t)

	cfg := &config.RuntimeConfig{
		Bind:         "0.0.0.0",
		Port:         config.DefaultPort,
		DataDir:      dir,
		ListenAddr:   fmt.Sprintf("0.0.0.0:%d", config.DefaultPort),
		DatabasePath: filepath.Join(dir, "wirehub.db"),
		JWTSecret:    "test-jwt-secret",
	}

	st, err := repo.New(cfg)
	if err != nil {
		t.Fatal(err)
	}

	priv, pub, err := tunnel.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	if err := st.Setup(repo.SetupInput{
		Endpoint:         "127.0.0.1",
		Subnet:           config.DefaultSubnet,
		AdminUsername:    "admin",
		AdminPassword:    "adminadmin",
		MTU:              config.DefaultMTU,
		StatusInterval:   config.DefaultStatusInterval,
		ListenPort:       listenPort,
		ServerPrivateKey: priv,
		ServerPublicKey:  pub,
	}); err != nil {
		t.Fatal(err)
	}

	settings, err := st.GetSettings()
	if err != nil {
		t.Fatal(err)
	}

	wgMgr, err := tunnel.NewManager(settings.HubIP, settings.DNSIP, nil, settings.ListenPort, settings.MTU)
	if err != nil {
		t.Fatal(err)
	}
	if err := wgMgr.ConfigureServer(settings.ServerPrivateKey, settings.ListenPort); err != nil {
		t.Fatal(err)
	}
	if err := wgMgr.Up(); err != nil {
		t.Fatal(err)
	}
	if err := vpnnetstack.EnableForwarding(wgMgr.Net()); err != nil {
		t.Fatal(err)
	}

	dnsServer := dnssvc.NewServer(settings.DNSIP, settings.UpstreamDNSResolvers())
	groupIDs := map[string]uint{}

	ensureGroup := func(name string) uint {
		if id, ok := groupIDs[name]; ok {
			return id
		}
		groups, _ := st.ListGroups()
		for _, g := range groups {
			if g.Name == name {
				groupIDs[name] = g.ID
				return g.ID
			}
		}
		g, err := st.CreateGroup(name, 0, 0)
		if err != nil {
			t.Fatalf("create group %q: %v", name, err)
		}
		groupIDs[name] = g.ID
		return g.ID
	}

	peers := make([]connectedPeer, 0, len(specs))
	for i, spec := range specs {
		groupName := spec.GroupName
		if groupName == "" {
			groupName = "default"
		}
		groupID := ensureGroup(groupName)

		peerIP, err := config.NthHostIP(config.DefaultSubnet, i+2)
		if err != nil {
			t.Fatal(err)
		}
		peerPriv, peerPub, err := tunnel.GenerateKeyPair()
		if err != nil {
			t.Fatal(err)
		}
		peer := repo.Peer{
			Name:       spec.Name,
			PublicKey:  peerPub,
			PrivateKey: peerPriv,
			WGIP:       peerIP,
			GroupID:    groupID,
			Enabled:    true,
			DNSName:    spec.Name,
		}
		if err := st.CreatePeer(&peer); err != nil {
			t.Fatal(err)
		}
		if err := ensurePeerDNSRecord(st, &peer); err != nil {
			t.Fatal(err)
		}
		if err := wgMgr.SyncPeer(runtime.WGPeer{
			ID: peer.ID, PublicKey: peer.PublicKey, WGIP: peer.WGIP,
			DNSName: peer.DNSName, GroupID: peer.GroupID, Enabled: peer.Enabled,
		}); err != nil {
			t.Fatal(err)
		}
		peers = append(peers, connectedPeer{Peer: peer})
	}

	for _, pair := range linkPairs {
		a := ensureGroup(pair[0])
		b := ensureGroup(pair[1])
		if err := st.UpsertGroupLink(a, b, true); err != nil {
			t.Fatal(err)
		}
	}

	if err := applyAccessFromStore(st, wgMgr); err != nil {
		t.Fatal(err)
	}

	bundle, err := buildSyncBundleFromStore(st)
	if err != nil {
		t.Fatal(err)
	}
	dnsServer.UpdateDNS(bundle.DNS, bundle.Peers)

	if err := dnsServer.StartOnNetstack(wgMgr.Net(), settings.DNSIP, 53); err != nil {
		t.Fatal(err)
	}

	env := &meshEnv{
		wgMgr:      wgMgr,
		dnsServer:  dnsServer,
		hubIP:      settings.HubIP,
		dnsIP:      settings.DNSIP,
		hubPubKey:  settings.ServerPublicKey,
		listenPort: listenPort,
		store:      st,
		peers:      peers,
	}
	return env, wgMgr.Net(), func() {
		for _, p := range env.peers {
			if p.Dev != nil {
				p.Dev.Close()
			}
		}
		if env.forwardProxy != nil {
			env.forwardProxy.Stop()
		}
		if env.mapProxy != nil {
			env.mapProxy.Stop()
		}
		_ = dnsServer.Stop()
		_ = wgMgr.Down()
	}
}

func (env *meshEnv) connectAll(t *testing.T) {
	t.Helper()
	for i := range env.peers {
		dev, tnet, err := startPeerClient(env.hubIP, env.hubPubKey, env.listenPort, env.peers[i].Peer)
		if err != nil {
			t.Fatal(err)
		}
		env.peers[i].Dev = dev
		env.peers[i].Net = tnet
		if err := waitHandshake(t, env.wgMgr, env.peers[i].Peer.PublicKey, 5*time.Second); err != nil {
			t.Fatal(err)
		}
	}
}

func (env *meshEnv) peerByName(name string) *connectedPeer {
	for i := range env.peers {
		if env.peers[i].Peer.Name == name {
			return &env.peers[i]
		}
	}
	return nil
}

func (env *meshEnv) setUnidirectionalLink(t *testing.T, fromGroup, toGroup string) {
	t.Helper()
	groups, err := env.store.ListGroups()
	if err != nil {
		t.Fatal(err)
	}
	var fromID, toID uint
	for _, g := range groups {
		if g.Name == fromGroup {
			fromID = g.ID
		}
		if g.Name == toGroup {
			toID = g.ID
		}
	}
	if fromID == 0 || toID == 0 {
		t.Fatalf("groups %q -> %q not found", fromGroup, toGroup)
	}
	if err := env.store.UpsertGroupLink(fromID, toID, false); err != nil {
		t.Fatal(err)
	}
	if err := applyAccessFromStore(env.store, env.wgMgr); err != nil {
		t.Fatal(err)
	}
}
