package api

import (
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	dnssvc "github.com/touken928/wirehub/internal/dns"
	"github.com/touken928/wirehub/internal/hostname"
	"github.com/touken928/wirehub/internal/store"
	"github.com/touken928/wirehub/internal/wg"
)

var errNetworkUnavailable = errors.New("network runtime is not running")

type NetworkController interface {
	Start() error
	Stop() error
	ReloadSettings() error
}

type Server struct {
	store      *store.Store
	networkMu  sync.RWMutex
	wg         *wg.Manager
	dns        *dnssvc.Server
	network    NetworkController
	statusStop chan struct{}
}

func New(st *store.Store) *Server {
	return &Server{store: st}
}

func (s *Server) SetNetworkController(nc NetworkController) {
	s.network = nc
}

func (s *Server) AttachNetwork(wgMgr *wg.Manager, dns *dnssvc.Server, statusInterval int) {
	s.networkMu.Lock()
	s.wg = wgMgr
	s.dns = dns
	s.networkMu.Unlock()
	s.StartStatusPoller(statusInterval)
	s.SyncAccessFilter()
}

func (s *Server) DetachNetwork() {
	s.StopStatusPoller()
	s.networkMu.Lock()
	s.wg = nil
	s.dns = nil
	s.networkMu.Unlock()
}

func (s *Server) StartStatusPoller(intervalSec int) {
	s.statusStop = make(chan struct{})
	go func() {
		ticker := time.NewTicker(time.Duration(intervalSec) * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				s.pollStats()
			case <-s.statusStop:
				return
			}
		}
	}()
}

func (s *Server) StopStatusPoller() {
	if s.statusStop != nil {
		close(s.statusStop)
		s.statusStop = nil
	}
}

func (s *Server) pollStats() {
	wgMgr, err := s.wgMgr()
	if err != nil {
		return
	}
	stats, err := wgMgr.GetStats()
	if err != nil {
		return
	}
	peers, err := s.store.ListPeers()
	if err != nil {
		return
	}
	for _, p := range peers {
		st, ok := stats[p.PublicKey]
		if !ok {
			continue
		}
		var hs int64
		if !st.LastHandshake.IsZero() {
			hs = st.LastHandshake.Unix()
		}
		_ = s.store.UpdatePeerStats(p.ID, hs, st.RxBytes, st.TxBytes)
	}
}

func (s *Server) wgMgr() (*wg.Manager, error) {
	s.networkMu.RLock()
	defer s.networkMu.RUnlock()
	if s.wg == nil {
		return nil, errNetworkUnavailable
	}
	return s.wg, nil
}

func (s *Server) dnsServer() (*dnssvc.Server, error) {
	s.networkMu.RLock()
	defer s.networkMu.RUnlock()
	if s.dns == nil {
		return nil, errNetworkUnavailable
	}
	return s.dns, nil
}

func (s *Server) ensureServerKeys(settings *store.Settings) error {
	if settings.ServerPrivateKey != "" && settings.ServerPublicKey != "" {
		return nil
	}
	priv, pub, err := wg.GenerateKeyPair()
	if err != nil {
		return err
	}
	settings.ServerPrivateKey = priv
	settings.ServerPublicKey = pub
	return s.store.UpdateSettings(settings)
}

func buildClientConfig(settings *store.Settings, peer *store.Peer) (string, error) {
	if settings.Endpoint == "" {
		return "", fmt.Errorf("server endpoint is not configured")
	}
	var b strings.Builder
	fmt.Fprintf(&b, "[Interface]\n")
	fmt.Fprintf(&b, "PrivateKey = %s\n", peer.PrivateKey)
	fmt.Fprintf(&b, "Address = %s/32\n", peer.WGIP)
	fmt.Fprintf(&b, "DNS = %s\n", strings.Join(settings.ClientDNS(), ", "))
	fmt.Fprintf(&b, "# Hub web UI: http://%s\n\n", hostname.HubFQDN())
	fmt.Fprintf(&b, "[Peer]\n")
	fmt.Fprintf(&b, "PublicKey = %s\n", settings.ServerPublicKey)
	fmt.Fprintf(&b, "Endpoint = %s:%d\n", settings.Endpoint, settings.ListenPort)
	fmt.Fprintf(&b, "PersistentKeepalive = 25\n")
	fmt.Fprintf(&b, "AllowedIPs = %s\n", settings.WGSubnet)
	return b.String(), nil
}
