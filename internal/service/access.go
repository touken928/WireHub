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
			Name:    p.Name,
			DNSName: p.DNSName,
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
			FromGroupID:   l.FromGroupID,
			ToGroupID:     l.ToGroupID,
			Bidirectional: l.Bidirectional,
		}
	}
	return out
}

func groupAccessList(groups []repo.PeerGroup) []domain.GroupAccess {
	out := make([]domain.GroupAccess, len(groups))
	for i, g := range groups {
		out[i] = domain.GroupAccess{
			ID:              g.ID,
			AllowIntraGroup: g.AllowIntraGroup,
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
	groups, err := h.Store.ListGroups()
	if err != nil {
		return err
	}
	policy, err := domain.BuildAccessPolicy(
		peerEndpoints(peers),
		groupLinkPairs(links),
		domain.NewGroupAccessPolicy(groupAccessList(groups)),
	)
	if err != nil {
		return err
	}
	wgMgr, err := h.wgManager()
	if err != nil {
		return err
	}
	wgMgr.SetAccessPolicy(policy)
	return nil
}

// SyncAccessFilter rebuilds group ACL rules on the running WireGuard stack.
func (h *Hub) SyncAccessFilter() {
	_ = h.buildAccessRules()
}
