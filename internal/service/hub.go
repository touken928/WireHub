package service

import (
	"errors"
	"sync"
	"time"

	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/repo"
	vpnruntime "github.com/touken928/wirehub/internal/vpn/runtime"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

var ErrNetworkUnavailable = errors.New("network runtime is not running")

// StatusPublisher receives a push after peer stats are polled from WireGuard.
type StatusPublisher interface {
	Publish()
}

// Hub manages VPN network lifecycle and dataplane access.
type Hub struct {
	app *App

	networkMu       sync.RWMutex
	network         NetworkRuntime
	dpMu            sync.RWMutex
	liveDP          vpnruntime.Dataplane
	statusStop      chan struct{}
	statusPublisher StatusPublisher
}

func (h *Hub) SetStatusPublisher(p StatusPublisher) {
	h.statusPublisher = p
}

func NewHub(a *App) *Hub {
	return &Hub{app: a}
}

func (h *Hub) SetNetworkRuntime(nc NetworkRuntime) {
	h.networkMu.Lock()
	h.network = nc
	h.networkMu.Unlock()
}

func (h *Hub) NetworkRuntime() NetworkRuntime {
	h.networkMu.RLock()
	defer h.networkMu.RUnlock()
	return h.network
}

func (h *Hub) dataplane() vpnruntime.Dataplane {
	h.dpMu.RLock()
	defer h.dpMu.RUnlock()
	return h.liveDP
}

func (h *Hub) onStarted(dp vpnruntime.Dataplane) {
	h.dpMu.Lock()
	h.liveDP = dp
	h.dpMu.Unlock()
	if bundle, err := h.app.loadSyncBundle(); err == nil {
		h.StartStatusPoller(bundle.Settings.StatusInterval)
	}
	_ = h.app.SyncAccessFilter()
}

func (h *Hub) onStopped() {
	h.StopStatusPoller()
	h.dpMu.Lock()
	h.liveDP = nil
	h.dpMu.Unlock()
}

func (h *Hub) StartStatusPoller(intervalSec int) {
	h.statusStop = make(chan struct{})
	go func() {
		ticker := time.NewTicker(time.Duration(intervalSec) * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				h.pollPeerStats()
			case <-h.statusStop:
				return
			}
		}
	}()
}

func (h *Hub) StopStatusPoller() {
	if h.statusStop != nil {
		close(h.statusStop)
		h.statusStop = nil
	}
}

func (h *Hub) SyncPortForwards() error {
	nc := h.NetworkRuntime()
	if nc == nil {
		return nil
	}
	return nc.SyncPortForwards()
}

func (h *Hub) SyncMaps() error {
	nc := h.NetworkRuntime()
	if nc == nil {
		return nil
	}
	if err := nc.SyncMaps(); err != nil {
		return err
	}
	return h.app.SyncAccessFilter()
}

func (h *Hub) pollPeerStats() {
	dp := h.dataplane()
	if dp == nil {
		return
	}
	stats, err := dp.GetStats()
	if err != nil {
		return
	}
	peers, err := h.app.Store.ListPeers()
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
		_ = h.app.Store.UpdatePeerStats(p.ID, hs, st.RxBytes, st.TxBytes)
	}
	if h.statusPublisher != nil {
		h.statusPublisher.Publish()
	}
}

func repoPeerToWG(p *repo.Peer) domainruntime.WGPeer {
	return domainruntime.WGPeer{
		ID:        p.ID,
		PublicKey: p.PublicKey,
		WGIP:      p.WGIP,
		DNSName:   p.DNSName,
		GroupID:   p.GroupID,
		Enabled:   p.Enabled,
	}
}

// PeerStats is re-exported for callers that need tunnel stats shape.
type PeerStats = tunnel.PeerStats
