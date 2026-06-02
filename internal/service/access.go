package service

import (
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
)

func peerEndpoints(peers []repo.Peer) []domain.PeerEndpoint {
	out := make([]domain.PeerEndpoint, len(peers))
	for i, p := range peers {
		out[i] = domain.PeerEndpoint{
			ID:      p.ID,
			WGIP:    p.WGIP,
			GroupID: p.GroupID,
			Enabled: p.Enabled,
		}
	}
	return out
}

func groupLinkPairs(links []repo.GroupLink) []domain.GroupLinkPair {
	out := make([]domain.GroupLinkPair, len(links))
	for i, l := range links {
		out[i] = domain.GroupLinkPair{
			FromGroupID: l.FromGroupID,
			ToGroupID:   l.ToGroupID,
		}
	}
	return out
}

func (h *Hub) buildAccessRules() error {
	peers, err := h.Store.ListPeers()
	if err != nil {
		return err
	}
	links, err := h.Store.ListGroupLinks()
	if err != nil {
		return err
	}
	rules, err := domain.BuildAccessRules(peerEndpoints(peers), groupLinkPairs(links))
	if err != nil {
		return err
	}
	wgMgr, err := h.wgManager()
	if err != nil {
		return err
	}
	wgMgr.SetAccessRules(rules)
	return nil
}

// SyncAccessFilter rebuilds group ACL rules on the running WireGuard stack.
func (h *Hub) SyncAccessFilter() {
	_ = h.buildAccessRules()
}
