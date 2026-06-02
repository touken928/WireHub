package integration

import (
	"fmt"
	"net/netip"
	"path/filepath"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/config"
	dnssvc "github.com/touken928/wirehub/internal/dns"
	"github.com/touken928/wirehub/internal/network"
	"github.com/touken928/wirehub/internal/store"
	"github.com/touken928/wirehub/internal/wg"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

type hubEnv struct {
	wgMgr       *wg.Manager
	dnsIP       string
	hubIP       string
	peerIP      string
	listenPort  int
	hubPubKey   string
	peerPrivKey string
	peerPubKey  string
}

type livePeer struct {
	Peer store.Peer
	Dev  *device.Device
	Net  *netstack.Net
}

type peerMeshEnv struct {
	wgMgr      *wg.Manager
	hubIP      string
	hubPubKey  string
	listenPort int
	store      *store.Store
	peers      []livePeer
}

func setupHub(t *testing.T) (*hubEnv, *netstack.Net, func()) {
	t.Helper()
	env, hubNet, cleanup := setupPeerMesh(t, []store.Peer{
		{Name: "touken"},
	})
	if len(env.peers) != 1 {
		t.Fatalf("expected 1 peer, got %d", len(env.peers))
	}
	legacy := &hubEnv{
		wgMgr:       env.wgMgr,
		dnsIP:       env.hubIP,
		hubIP:       env.hubIP,
		peerIP:      env.peers[0].Peer.WGIP,
		listenPort:  env.listenPort,
		hubPubKey:   env.hubPubKey,
		peerPrivKey: env.peers[0].Peer.PrivateKey,
		peerPubKey:  env.peers[0].Peer.PublicKey,
	}
	return legacy, hubNet, cleanup
}

func setupPeerMesh(t *testing.T, specs []store.Peer) (*peerMeshEnv, *netstack.Net, func()) {
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

	st, err := store.New(cfg)
	if err != nil {
		t.Fatal(err)
	}

	priv, pub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	if err := st.Setup(store.SetupInput{
		Endpoint:         "127.0.0.1",
		Subnet:           config.DefaultSubnet,
		AdminUsername:    "admin",
		AdminPassword:    "admin",
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

	wgMgr, err := wg.NewManager(settings.HubIP, settings.DNSIP, settings.ListenPort, settings.MTU)
	if err != nil {
		t.Fatal(err)
	}
	if err := wgMgr.ConfigureServer(settings.ServerPrivateKey, settings.ListenPort); err != nil {
		t.Fatal(err)
	}
	if err := wgMgr.Up(); err != nil {
		t.Fatal(err)
	}
	if err := network.EnableForwarding(wgMgr.Net()); err != nil {
		t.Fatal(err)
	}

	dnsServer := dnssvc.NewServer(st, settings.HubIP, settings.DNSIP, settings.UpstreamDNSOrDefault())

	live := make([]livePeer, 0, len(specs))
	for i, spec := range specs {
		peerIP, err := config.NthHostIP(config.DefaultSubnet, i+2)
		if err != nil {
			t.Fatal(err)
		}
		peerPriv, peerPub, err := wg.GenerateKeyPair()
		if err != nil {
			t.Fatal(err)
		}
		peer := store.Peer{
			Name:          spec.Name,
			PublicKey:     peerPub,
			PrivateKey:    peerPriv,
			WGIP:          peerIP,
			Enabled:       true,
			DNSName:       spec.Name,
			AccessExclude: append([]string(nil), spec.AccessExclude...),
		}
		if !spec.Enabled && spec.Name != "" {
			peer.Enabled = spec.Enabled
		}
		if err := st.CreatePeer(&peer); err != nil {
			t.Fatal(err)
		}
		_ = dnsServer.RegisterPeer(&peer)
		if err := wgMgr.SyncPeer(&peer); err != nil {
			t.Fatal(err)
		}
		live = append(live, livePeer{Peer: peer})
	}

	if err := reloadAccessRules(st, wgMgr); err != nil {
		t.Fatal(err)
	}

	if err := dnsServer.StartOnNetstack(wgMgr.Net(), settings.DNSIP, 53); err != nil {
		t.Fatal(err)
	}

	env := &peerMeshEnv{
		wgMgr:      wgMgr,
		hubIP:      settings.HubIP,
		hubPubKey:  settings.ServerPublicKey,
		listenPort: listenPort,
		store:      st,
		peers:      live,
	}
	return env, wgMgr.Net(), func() {
		for _, p := range env.peers {
			if p.Dev != nil {
				p.Dev.Close()
			}
		}
		_ = dnsServer.Stop()
		_ = wgMgr.Down()
	}
}

func reloadAccessRules(st *store.Store, wgMgr *wg.Manager) error {
	peers, err := st.ListPeers()
	if err != nil {
		return err
	}
	return applyAccessRules(wgMgr, peers)
}

func (env *peerMeshEnv) connectPeers(t *testing.T) {
	t.Helper()
	for i := range env.peers {
		dev, tnet, err := startWireGuardClient(env.hubIP, env.hubPubKey, env.listenPort, env.peers[i].Peer)
		if err != nil {
			t.Fatal(err)
		}
		env.peers[i].Dev = dev
		env.peers[i].Net = tnet
		if err := waitForHandshake(t, env.wgMgr, env.peers[i].Peer.PublicKey, 5*time.Second); err != nil {
			t.Fatal(err)
		}
	}
}

func (env *peerMeshEnv) peerNamed(name string) *livePeer {
	for i := range env.peers {
		if env.peers[i].Peer.Name == name {
			return &env.peers[i]
		}
	}
	return nil
}

func startWireGuardClient(hubIP, hubPubKey string, listenPort int, peer store.Peer) (*device.Device, *netstack.Net, error) {
	clientTun, clientNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr(peer.WGIP)},
		[]netip.Addr{netip.MustParseAddr(hubIP)},
		1420,
	)
	if err != nil {
		return nil, nil, err
	}

	clientDev := device.NewDevice(clientTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	peerPrivHex, err := wg.KeyToHex(peer.PrivateKey)
	if err != nil {
		return nil, nil, err
	}
	hubPubHex, err := wg.KeyToHex(hubPubKey)
	if err != nil {
		return nil, nil, err
	}
	cfg := fmt.Sprintf(`private_key=%s
public_key=%s
endpoint=127.0.0.1:%d
allowed_ip=%s
persistent_keepalive_interval=1
`, peerPrivHex, hubPubHex, listenPort, config.DefaultSubnet)
	if err := clientDev.IpcSet(cfg); err != nil {
		return nil, nil, err
	}
	if err := clientDev.Up(); err != nil {
		return nil, nil, err
	}
	return clientDev, clientNet, nil
}

func startWireGuardClientLegacy(t *testing.T, env *hubEnv) (*device.Device, *netstack.Net, error) {
	t.Helper()
	return startWireGuardClient(env.hubIP, env.hubPubKey, env.listenPort, store.Peer{
		PrivateKey: env.peerPrivKey,
		PublicKey:  env.peerPubKey,
		WGIP:       env.peerIP,
	})
}
