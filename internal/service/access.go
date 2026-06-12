package service

import (
	"github.com/touken928/wirehub/internal/domain/policy"
	"github.com/touken928/wirehub/internal/repo"
)

func peerEndpoints(peers []repo.Peer) []policy.PeerEndpoint {
	out := make([]policy.PeerEndpoint, len(peers))
	for i, p := range peers {
		out[i] = policy.PeerEndpoint{
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

func groupLinkPairs(links []repo.GroupLink) []policy.GroupLinkPair {
	out := make([]policy.GroupLinkPair, len(links))
	for i, l := range links {
		out[i] = policy.GroupLinkPair{
			FromGroupID:   l.FromGroupID,
			ToGroupID:     l.ToGroupID,
			Bidirectional: l.Bidirectional,
		}
	}
	return out
}

func groupAccessList(groups []repo.PeerGroup) []policy.GroupAccess {
	out := make([]policy.GroupAccess, len(groups))
	for i, g := range groups {
		out[i] = policy.GroupAccess{
			ID:              g.ID,
			AllowIntraGroup: g.AllowIntraGroup,
		}
	}
	return out
}

func (a *App) buildAccessPolicySpec() (policy.AccessPolicySpec, error) {
	peers, err := a.Store.ListPeers()
	if err != nil {
		return policy.AccessPolicySpec{}, err
	}
	links, err := a.Store.ListGroupLinks()
	if err != nil {
		return policy.AccessPolicySpec{}, err
	}
	groups, err := a.Store.ListGroups()
	if err != nil {
		return policy.AccessPolicySpec{}, err
	}
	return a.buildAccessPolicySpecFrom(peers, groupLinkPairs(links), groupAccessList(groups))
}

func (a *App) buildAccessPolicySpecFrom(peers []repo.Peer, linkPairs []policy.GroupLinkPair, groupAccess []policy.GroupAccess) (policy.AccessPolicySpec, error) {
	mapPolicy, err := a.buildMapAccessPolicy()
	if err != nil {
		return policy.AccessPolicySpec{}, err
	}
	return policy.BuildAccessPolicySpec(
		peerEndpoints(peers),
		linkPairs,
		policy.NewGroupAccessPolicy(groupAccess),
		mapPolicy,
	)
}

// SyncAccessFilter rebuilds group ACL rules on the running dataplane.
func (a *App) SyncAccessFilter() error {
	spec, err := a.buildAccessPolicySpec()
	if err != nil {
		return err
	}
	dp := a.Hub.dataplane()
	if dp == nil {
		return nil
	}
	return dp.ApplyPolicy(spec)
}

func (a *App) buildMapAccessPolicy() (policy.MapAccessPolicy, error) {
	details, err := a.Store.ListMapDetails()
	if err != nil {
		return policy.MapAccessPolicy{}, err
	}
	maps := make([]policy.MapAccess, 0, len(details))
	for _, d := range details {
		maps = append(maps, policy.NewMapAccess(d.VirtualIP, d.AllowedGroups))
	}
	return policy.NewMapAccessPolicy(maps), nil
}
