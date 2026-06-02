package runtime

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/touken928/wirehub/internal/api"
	"github.com/touken928/wirehub/internal/config"
	dnssvc "github.com/touken928/wirehub/internal/dns"
	"github.com/touken928/wirehub/internal/network"
	"github.com/touken928/wirehub/internal/store"
	"github.com/touken928/wirehub/internal/wg"
)

// Network manages WireGuard, DNS, status polling, and tunnel web serving.
type Network struct {
	mu          sync.Mutex
	cfg         *config.RuntimeConfig
	store       *store.Store
	api         *api.Server
	httpHandler http.Handler
	wgMgr       *wg.Manager
	dnsServer   *dnssvc.Server
	tunnelSrv   *http.Server
}

func NewNetwork(cfg *config.RuntimeConfig, st *store.Store, apiSvc *api.Server, handler http.Handler) *Network {
	return &Network{
		cfg:         cfg,
		store:       st,
		api:         apiSvc,
		httpHandler: handler,
	}
}

func (n *Network) Start() error {
	n.mu.Lock()
	defer n.mu.Unlock()

	if n.wgMgr != nil {
		return nil
	}

	settings, err := n.store.GetSettings()
	if err != nil {
		return fmt.Errorf("settings: %w", err)
	}

	mtu := settings.MTU
	if mtu == 0 {
		mtu = config.DefaultMTU
	}
	statusInterval := settings.StatusInterval
	if statusInterval == 0 {
		statusInterval = config.DefaultStatusInterval
	}

	wgMgr, err := wg.NewManager(settings.HubIP, settings.DNSIP, settings.ListenPort, mtu)
	if err != nil {
		return fmt.Errorf("wireguard: %w", err)
	}

	if err := wgMgr.ConfigureServer(settings.ServerPrivateKey, settings.ListenPort); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("configure wireguard: %w", err)
	}
	if err := wgMgr.Up(); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("wireguard up: %w", err)
	}

	peers, err := n.store.ListPeers()
	if err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("list peers: %w", err)
	}
	if err := wgMgr.SyncAll(peers); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("sync peers: %w", err)
	}

	if err := network.EnableForwarding(wgMgr.Net()); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("netstack forwarding: %w", err)
	}

	dnsServer := dnssvc.NewServer(n.store, settings.HubIP, settings.DNSIP, settings.UpstreamDNSOrDefault())
	if err := dnsServer.StartOnNetstack(wgMgr.Net(), settings.DNSIP, 53); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("dns server: %w", err)
	}

	n.api.AttachNetwork(wgMgr, dnsServer, statusInterval)

	tunnelSrv, err := network.StartHubWebServer(wgMgr.Net(), settings.HubIP, n.cfg.Port, n.httpHandler)
	if err != nil {
		n.api.DetachNetwork()
		_ = dnsServer.Stop()
		_ = wgMgr.Close()
		return fmt.Errorf("tunnel web: %w", err)
	}

	n.wgMgr = wgMgr
	n.dnsServer = dnsServer
	n.tunnelSrv = tunnelSrv
	log.Printf("WireHub network runtime started (WG port %d)", settings.ListenPort)
	return nil
}

func (n *Network) Stop() error {
	n.mu.Lock()
	defer n.mu.Unlock()

	if n.wgMgr == nil {
		return nil
	}

	n.api.DetachNetwork()

	if n.wgMgr != nil {
		_ = n.wgMgr.Down()
	}

	if n.dnsServer != nil {
		_ = n.dnsServer.Stop()
		n.dnsServer = nil
	}

	if n.tunnelSrv != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = n.tunnelSrv.Shutdown(ctx)
		cancel()
		n.tunnelSrv = nil
	}

	n.closeWireGuard()
	n.wgMgr = nil
	log.Printf("WireHub network runtime stopped")
	return nil
}

func (n *Network) closeWireGuard() {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("wireguard close recovered: %v", r)
		}
	}()
	if n.wgMgr != nil {
		_ = n.wgMgr.Close()
	}
}

func (n *Network) Running() bool {
	n.mu.Lock()
	defer n.mu.Unlock()
	return n.wgMgr != nil
}

// ReloadSettings restarts the network stack so MTU and DNS upstream changes take effect.
func (n *Network) ReloadSettings() error {
	n.mu.Lock()
	running := n.wgMgr != nil
	n.mu.Unlock()
	if !running {
		return nil
	}
	if err := n.Stop(); err != nil {
		return err
	}
	return n.Start()
}
