package runtime

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/core"
	vpndns "github.com/touken928/wirehub/internal/vpn/dns"
	"github.com/touken928/wirehub/internal/vpn/ingress"
	"github.com/touken928/wirehub/internal/vpn/netstack"
	dompolicy "github.com/touken928/wirehub/internal/domain/policy"
	vpnpolicy "github.com/touken928/wirehub/internal/vpn/policy"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// Stack manages WireGuard, DNS, tunnel web, and L4 ingress on the hub netstack.
type Stack struct {
	mu           sync.Mutex
	cfg          *config.RuntimeConfig
	cb           Callbacks
	httpHandler  http.Handler
	tunnelMgr    *tunnel.Manager
	dnsServer    *vpndns.Server
	tunnelSrv    *http.Server
	forwardProxy *ingress.ForwardProxy
	mapProxy     *ingress.MapProxy
	startBundle  runtime.SyncBundle
}

func NewStack(cfg *config.RuntimeConfig, cb Callbacks, handler http.Handler) *Stack {
	return &Stack{
		cfg:         cfg,
		cb:          cb,
		httpHandler: handler,
	}
}

func (s *Stack) Start(bundle runtime.SyncBundle) error {
	s.mu.Lock()
	if s.tunnelMgr != nil {
		s.mu.Unlock()
		return nil
	}

	wgPort := s.cfg.Port
	mapVIP := parseMapVIPs(bundle.Maps)
	tunnelMgr, err := tunnel.NewManager(bundle.Settings.HubIP, bundle.Settings.DNSIP, mapVIP, wgPort, bundle.Settings.MTU)
	if err != nil {
		s.mu.Unlock()
		return fmt.Errorf("wireguard: %w", err)
	}

	if err := tunnelMgr.ConfigureServer(bundle.Settings.ServerPrivateKey, wgPort); err != nil {
		_ = tunnelMgr.Close()
		s.mu.Unlock()
		return fmt.Errorf("configure wireguard: %w", err)
	}
	if err := tunnelMgr.Up(); err != nil {
		_ = tunnelMgr.Close()
		s.mu.Unlock()
		return fmt.Errorf("wireguard up: %w", err)
	}

	if err := tunnelMgr.SyncAll(bundle.Peers); err != nil {
		_ = tunnelMgr.Close()
		s.mu.Unlock()
		return fmt.Errorf("sync peers: %w", err)
	}

	if err := netstack.EnableForwarding(tunnelMgr.Net()); err != nil {
		_ = tunnelMgr.Close()
		s.mu.Unlock()
		return fmt.Errorf("netstack forwarding: %w", err)
	}

	dnsServer := vpndns.NewServer(bundle.Settings.DNSIP, bundle.Settings.UpstreamDNS)
	dnsServer.UpdateDNS(bundle.DNS, bundle.Peers)
	if err := dnsServer.StartOnNetstack(tunnelMgr.Net(), bundle.Settings.DNSIP, core.HubDNSPort); err != nil {
		_ = tunnelMgr.Close()
		s.mu.Unlock()
		return fmt.Errorf("dns server: %w", err)
	}

	tunnelSrv, err := ingress.StartWebServer(tunnelMgr.Net(), bundle.Settings.HubIP, core.HubTunnelWebPort, s.httpHandler)
	if err != nil {
		_ = dnsServer.Stop()
		_ = tunnelMgr.Close()
		s.mu.Unlock()
		return fmt.Errorf("tunnel web: %w", err)
	}

	s.tunnelMgr = tunnelMgr
	s.dnsServer = dnsServer
	s.tunnelSrv = tunnelSrv
	s.startBundle = bundle

	if err := s.applyIngressLocked(bundle); err != nil {
		s.rollbackStart(dnsServer, tunnelSrv, tunnelMgr)
		s.mu.Unlock()
		return err
	}

	tunnelMgr.SetAccessPolicy(vpnpolicy.Apply(bundle.Policy))
	s.mu.Unlock()

	// Callback may re-enter Stack (e.g. SyncAccessFilter → ApplyPolicy); must not hold s.mu.
	s.cb.OnStarted(s)

	log.Printf("WireHub VPN stack started (WG UDP port %d, client endpoint port %d)", wgPort, bundle.Settings.ListenPort)
	return nil
}

func (s *Stack) applyIngressLocked(bundle runtime.SyncBundle) error {
	if s.forwardProxy == nil {
		m, err := ingress.NewForwardProxy(s.tunnelMgr.Net(), bundle.Settings.HubIP, bundle.Settings.WGSubnet, s.dnsServer)
		if err != nil {
			return fmt.Errorf("forward proxy: %w", err)
		}
		s.forwardProxy = m
	}
	if s.mapProxy == nil {
		m, err := ingress.NewMapProxy(s.tunnelMgr.Net(), bundle.Settings.WGSubnet, s.dnsServer)
		if err != nil {
			return fmt.Errorf("map proxy: %w", err)
		}
		s.mapProxy = m
	}
	fwd := ingressForwardRules(bundle.Forwards)
	s.tunnelMgr.ReserveHubPorts(ingress.ReservedHubPorts(core.HubTunnelWebPort, fwd))
	if err := s.forwardProxy.Apply(fwd); err != nil {
		return fmt.Errorf("port forwards: %w", err)
	}
	if err := s.tunnelMgr.EnsureMapVIPs(parseMapVIPs(bundle.Maps)); err != nil {
		return fmt.Errorf("map vips: %w", err)
	}
	s.tunnelMgr.SetMapVIPs(parseMapVIPs(bundle.Maps))
	if err := s.mapProxy.Apply(ingressMapRules(bundle.Maps), bundle.Peers); err != nil {
		return fmt.Errorf("maps: %w", err)
	}
	return nil
}

func (s *Stack) rollbackStart(dnsServer *vpndns.Server, tunnelSrv *http.Server, tunnelMgr *tunnel.Manager) {
	_ = dnsServer.Stop()
	shutCtx, shutCancel := context.WithTimeout(context.Background(), 5*time.Second)
	_ = tunnelSrv.Shutdown(shutCtx)
	shutCancel()
	if s.forwardProxy != nil {
		s.forwardProxy.Stop()
		s.forwardProxy = nil
	}
	_ = tunnelMgr.Close()
	s.tunnelMgr = nil
	s.dnsServer = nil
	s.tunnelSrv = nil
}

func (s *Stack) HubListenPort() int {
	return s.cfg.Port
}

func (s *Stack) loadBundle() (runtime.SyncBundle, error) {
	return s.cb.LoadSyncBundle()
}

func (s *Stack) SyncMaps() error {
	bundle, err := s.loadBundle()
	if err != nil {
		return err
	}
	return s.FullSync(bundle)
}

func (s *Stack) SyncPortForwards() error {
	bundle, err := s.loadBundle()
	if err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tunnelMgr == nil {
		return nil
	}
	fwd := ingressForwardRules(bundle.Forwards)
	s.tunnelMgr.ReserveHubPorts(ingress.ReservedHubPorts(core.HubTunnelWebPort, fwd))
	if s.forwardProxy == nil {
		return nil
	}
	return s.forwardProxy.Apply(fwd)
}

func (s *Stack) FullSync(bundle runtime.SyncBundle) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tunnelMgr == nil {
		return nil
	}
	if s.dnsServer != nil {
		s.dnsServer.UpdateDNS(bundle.DNS, bundle.Peers)
	}
	if err := s.tunnelMgr.SyncAll(bundle.Peers); err != nil {
		return err
	}
	if err := s.applyIngressLocked(bundle); err != nil {
		return err
	}
	s.tunnelMgr.SetAccessPolicy(vpnpolicy.Apply(bundle.Policy))
	return nil
}

func (s *Stack) SyncPeer(peer runtime.WGPeer) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tunnelMgr == nil {
		return nil
	}
	return s.tunnelMgr.SyncPeer(peer)
}

func (s *Stack) RemovePeer(publicKey string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tunnelMgr == nil {
		return nil
	}
	return s.tunnelMgr.RemovePeer(publicKey)
}

func (s *Stack) ApplyPolicy(spec dompolicy.AccessPolicySpec) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tunnelMgr == nil {
		return nil
	}
	s.tunnelMgr.SetAccessPolicy(vpnpolicy.Apply(spec))
	return nil
}

func (s *Stack) UpdateDNS(catalog runtime.DNSCatalog, peers []runtime.WGPeer) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.dnsServer == nil {
		return nil
	}
	s.dnsServer.UpdateDNS(catalog, peers)
	return nil
}

// SetDNSUpstream updates upstream resolvers on the live DNS server.
func (s *Stack) SetDNSUpstream(upstream []string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.dnsServer != nil {
		s.dnsServer.SetUpstream(upstream)
	}
}

func (s *Stack) GetStats() (map[string]tunnel.PeerStats, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tunnelMgr == nil {
		return nil, nil
	}
	return s.tunnelMgr.GetStats()
}

func (s *Stack) Stop() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.tunnelMgr == nil {
		return nil
	}

	s.cb.OnStopped()

	if s.tunnelMgr != nil {
		_ = s.tunnelMgr.Down()
	}

	if s.dnsServer != nil {
		_ = s.dnsServer.Stop()
		s.dnsServer = nil
	}

	if s.tunnelSrv != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = s.tunnelSrv.Shutdown(ctx)
		cancel()
		s.tunnelSrv = nil
	}

	if s.forwardProxy != nil {
		s.forwardProxy.Stop()
		s.forwardProxy = nil
	}

	if s.mapProxy != nil {
		s.mapProxy.Stop()
		s.mapProxy = nil
	}

	s.closeTunnel()
	s.tunnelMgr = nil
	log.Printf("WireHub VPN stack stopped")
	return nil
}

func (s *Stack) closeTunnel() {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("wireguard close recovered: %v", r)
		}
	}()
	if s.tunnelMgr != nil {
		_ = s.tunnelMgr.Close()
	}
}

func (s *Stack) Running() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.tunnelMgr != nil
}

// ReloadSettings restarts the VPN stack so MTU changes take effect.
func (s *Stack) ReloadSettings() error {
	s.mu.Lock()
	running := s.tunnelMgr != nil
	s.mu.Unlock()
	if !running {
		return nil
	}
	bundle, err := s.loadBundle()
	if err != nil {
		return err
	}
	if err := s.Stop(); err != nil {
		return err
	}
	return s.Start(bundle)
}

// Ensure Stack implements Dataplane.
var _ Dataplane = (*Stack)(nil)
