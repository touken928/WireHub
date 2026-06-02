package service

import (
	"errors"
	"sync"
	"time"

	dnssvc "github.com/touken928/wirehub/internal/vpn/dns"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/wg"
)

var ErrNetworkUnavailable = errors.New("network runtime is not running")

// Hub is the application service: persistence orchestration and optional live network attachments.
type Hub struct {
	Store *repo.Store

	networkMu  sync.RWMutex
	wg         *wg.Manager
	dns        *dnssvc.Server
	network    NetworkRuntime
	statusStop chan struct{}
}

func NewHub(st *repo.Store) *Hub {
	return &Hub{Store: st}
}

func (h *Hub) SetNetworkRuntime(nc NetworkRuntime) {
	h.network = nc
}

func (h *Hub) NetworkRuntime() NetworkRuntime {
	return h.network
}

func (h *Hub) AttachNetwork(wgMgr *wg.Manager, dns *dnssvc.Server, statusIntervalSec int) {
	h.networkMu.Lock()
	h.wg = wgMgr
	h.dns = dns
	h.networkMu.Unlock()
	h.StartStatusPoller(statusIntervalSec)
	h.SyncAccessFilter()
}

func (h *Hub) DetachNetwork() {
	h.StopStatusPoller()
	h.networkMu.Lock()
	h.wg = nil
	h.dns = nil
	h.networkMu.Unlock()
}

func (h *Hub) wgManager() (*wg.Manager, error) {
	h.networkMu.RLock()
	defer h.networkMu.RUnlock()
	if h.wg == nil {
		return nil, ErrNetworkUnavailable
	}
	return h.wg, nil
}

func (h *Hub) dnsServer() (*dnssvc.Server, error) {
	h.networkMu.RLock()
	defer h.networkMu.RUnlock()
	if h.dns == nil {
		return nil, ErrNetworkUnavailable
	}
	return h.dns, nil
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

func (h *Hub) pollPeerStats() {
	wgMgr, err := h.wgManager()
	if err != nil {
		return
	}
	stats, err := wgMgr.GetStats()
	if err != nil {
		return
	}
	peers, err := h.Store.ListPeers()
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
		_ = h.Store.UpdatePeerStats(p.ID, hs, st.RxBytes, st.TxBytes)
	}
}
