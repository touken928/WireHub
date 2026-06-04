package vpn

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
	vpndns "github.com/touken928/wirehub/internal/vpn/dns"
	"github.com/touken928/wirehub/internal/vpn/filter"
	"github.com/touken928/wirehub/internal/vpn/filter/l4"
	"github.com/touken928/wirehub/internal/vpn/wg"
)

// Stack manages WireGuard, DNS, status polling, and tunnel web serving.
type Stack struct {
	mu           sync.Mutex
	cfg          *config.RuntimeConfig
	repo         *repo.Store
	hub          *service.Hub
	httpHandler  http.Handler
	wgMgr        *wg.Manager
	dnsServer    *vpndns.Server
	tunnelSrv    *http.Server
	forwardProxy *l4.ForwardProxy
	mapProxy  *l4.MapProxy
}

func NewStack(cfg *config.RuntimeConfig, st *repo.Store, hub *service.Hub, handler http.Handler) *Stack {
	return &Stack{
		cfg:         cfg,
		repo:        st,
		hub:         hub,
		httpHandler: handler,
	}
}

func (s *Stack) Start() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.wgMgr != nil {
		return nil
	}

	settings, err := s.repo.GetSettings()
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

	wgPort := s.cfg.Port
	mapVIPStrs, _ := s.repo.MapVirtualIPs()
	mapVIP := l4.ParseMapVIP(mapVIPStrs)
	wgMgr, err := wg.NewManager(settings.HubIP, settings.DNSIP, mapVIP, wgPort, mtu)
	if err != nil {
		return fmt.Errorf("wireguard: %w", err)
	}

	if err := wgMgr.ConfigureServer(settings.ServerPrivateKey, wgPort); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("configure wireguard: %w", err)
	}
	if err := wgMgr.Up(); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("wireguard up: %w", err)
	}

	peers, err := s.repo.ListPeers()
	if err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("list peers: %w", err)
	}
	if err := wgMgr.SyncAll(peers); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("sync peers: %w", err)
	}

	if err := filter.EnableForwarding(wgMgr.Net()); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("netstack forwarding: %w", err)
	}

	dnsServer := vpndns.NewServer(s.repo, settings.HubIP, settings.DNSIP, settings.UpstreamDNSResolvers())
	if err := dnsServer.StartOnNetstack(wgMgr.Net(), settings.DNSIP, l4.HubDNSPort); err != nil {
		_ = wgMgr.Close()
		return fmt.Errorf("dns server: %w", err)
	}

	s.hub.AttachNetwork(wgMgr, dnsServer, statusInterval)

	tunnelSrv, err := l4.StartWebServer(wgMgr.Net(), settings.HubIP, l4.HubTunnelWebPort, s.httpHandler)
	if err != nil {
		s.hub.DetachNetwork()
		_ = dnsServer.Stop()
		_ = wgMgr.Close()
		return fmt.Errorf("tunnel web: %w", err)
	}

	s.wgMgr = wgMgr
	s.dnsServer = dnsServer
	s.tunnelSrv = tunnelSrv

	if err := s.applyPortForwards(settings.HubIP); err != nil {
		s.hub.DetachNetwork()
		_ = dnsServer.Stop()
		shutCtx, shutCancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = tunnelSrv.Shutdown(shutCtx)
		shutCancel()
		_ = wgMgr.Close()
		s.wgMgr = nil
		s.dnsServer = nil
		s.tunnelSrv = nil
		return fmt.Errorf("port forwards: %w", err)
	}

	if err := s.applyMaps(settings); err != nil {
		s.hub.DetachNetwork()
		_ = dnsServer.Stop()
		shutCtx, shutCancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = tunnelSrv.Shutdown(shutCtx)
		shutCancel()
		if s.forwardProxy != nil {
			s.forwardProxy.Stop()
			s.forwardProxy = nil
		}
		_ = wgMgr.Close()
		s.wgMgr = nil
		s.dnsServer = nil
		s.tunnelSrv = nil
		return fmt.Errorf("maps: %w", err)
	}

	log.Printf("WireHub VPN stack started (WG UDP port %d, client endpoint port %d)", wgPort, settings.ListenPort)
	return nil
}

func (s *Stack) HubListenPort() int {
	return s.cfg.Port
}

func (s *Stack) SyncMaps() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.wgMgr == nil || s.dnsServer == nil {
		return nil
	}
	settings, err := s.repo.GetSettings()
	if err != nil {
		return err
	}
	return s.applyMaps(settings)
}

func (s *Stack) applyMaps(settings *repo.Settings) error {
	if s.mapProxy == nil {
		m, err := l4.NewMapProxy(s.wgMgr.Net(), settings.WGSubnet, s.dnsServer)
		if err != nil {
			return err
		}
		s.mapProxy = m
	}
	rules, err := service.MapRulesFromStore(s.repo)
	if err != nil {
		return err
	}
	if err := s.mapProxy.Apply(rules); err != nil {
		return err
	}
	s.hub.SyncAccessFilter()
	return nil
}

func (s *Stack) SyncPortForwards() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.wgMgr == nil || s.dnsServer == nil {
		return nil
	}
	settings, err := s.repo.GetSettings()
	if err != nil {
		return err
	}
	return s.applyPortForwards(settings.HubIP)
}

func (s *Stack) applyPortForwards(hubIP string) error {
	settings, err := s.repo.GetSettings()
	if err != nil {
		return err
	}
	if s.forwardProxy == nil {
		m, err := l4.NewForwardProxy(s.wgMgr.Net(), hubIP, settings.WGSubnet, s.dnsServer)
		if err != nil {
			return err
		}
		s.forwardProxy = m
	}
	rules, err := s.repo.ListPortForwards()
	if err != nil {
		return err
	}
	runtimeRules := forwardRulesFromRepo(rules)
	if s.wgMgr != nil {
		s.wgMgr.ReserveHubPorts(l4.ReservedHubPorts(l4.HubTunnelWebPort, runtimeRules))
	}
	return s.forwardProxy.Apply(runtimeRules)
}

func forwardRulesFromRepo(rules []repo.PortForward) []l4.ForwardRule {
	out := make([]l4.ForwardRule, 0, len(rules))
	for _, r := range rules {
		out = append(out, l4.ForwardRule{
			ID:         r.ID,
			ListenPort: r.ListenPort,
			Protocol:   r.Protocol,
			TargetHost: r.TargetHost,
			TargetPort: r.TargetPort,
		})
	}
	return out
}

func (s *Stack) Stop() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.wgMgr == nil {
		return nil
	}

	s.hub.DetachNetwork()

	if s.wgMgr != nil {
		_ = s.wgMgr.Down()
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

	s.closeWireGuard()
	s.wgMgr = nil
	log.Printf("WireHub VPN stack stopped")
	return nil
}

func (s *Stack) closeWireGuard() {
	defer func() {
		if r := recover(); r != nil {
			log.Printf("wireguard close recovered: %v", r)
		}
	}()
	if s.wgMgr != nil {
		_ = s.wgMgr.Close()
	}
}

func (s *Stack) Running() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.wgMgr != nil
}

// ReloadSettings restarts the VPN stack so MTU and listen-port changes take effect.
func (s *Stack) ReloadSettings() error {
	s.mu.Lock()
	running := s.wgMgr != nil
	s.mu.Unlock()
	if !running {
		return nil
	}
	if err := s.Stop(); err != nil {
		return err
	}
	return s.Start()
}
