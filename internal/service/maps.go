package service

import (
	"net/netip"

	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/filter/l4"
)

func (h *Hub) SyncMaps() error {
	if h.network == nil {
		return nil
	}
	type mapSyncer interface {
		SyncMaps() error
	}
	if rs, ok := h.network.(mapSyncer); ok {
		return rs.SyncMaps()
	}
	return nil
}

// MapRulesFromStore builds runtime map rules from persisted map details.
func MapRulesFromStore(st *repo.Store) ([]l4.MapRule, error) {
	details, err := st.ListMapDetails()
	if err != nil {
		return nil, err
	}
	out := make([]l4.MapRule, 0, len(details))
	for _, d := range details {
		vip, err := netip.ParseAddr(d.VirtualIP)
		if err != nil {
			continue
		}
		allowed := domain.AllowedGroupIDSet(d.AllowedGroups)
		rule := l4.MapRule{
			ID:          d.ID,
			Slug:        d.Slug,
			TargetHost:  d.TargetHost,
			VirtualIP:   vip,
			AllowedPeer: mapAllowedPeerFunc(st, allowed),
		}
		out = append(out, rule)
	}
	return out, nil
}

func mapAllowedPeerFunc(st *repo.Store, allowed map[uint]struct{}) func(netip.Addr) bool {
	return func(peerWGIP netip.Addr) bool {
		peer, err := st.GetPeerByWGIP(peerWGIP.String())
		if err != nil {
			return false
		}
		return domain.GroupInAllowedSet(allowed, peer.GroupID)
	}
}
