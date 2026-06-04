package dns

import (
	"net/netip"
	"sync"

	"github.com/touken928/wirehub/internal/domain/runtime"
)

type catalogState struct {
	catalog   runtime.DNSCatalog
	peerGroup map[netip.Addr]uint
}

type catalogStore struct {
	mu    sync.RWMutex
	state catalogState
}

func (c *catalogStore) update(catalog runtime.DNSCatalog, peers []runtime.WGPeer) {
	pg := make(map[netip.Addr]uint, len(peers))
	for _, p := range peers {
		if !p.Enabled || p.GroupID == 0 {
			continue
		}
		ip, err := netip.ParseAddr(p.WGIP)
		if err != nil {
			continue
		}
		pg[ip] = p.GroupID
	}
	c.mu.Lock()
	c.state = catalogState{catalog: catalog, peerGroup: pg}
	c.mu.Unlock()
}

func (c *catalogStore) snapshot() catalogState {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.state
}
