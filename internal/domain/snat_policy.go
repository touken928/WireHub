package domain

import (
	"net/netip"

	"github.com/touken928/wirehub/internal/vpn/filter/l4"
)

// BuildTransparentTable configures hub transparent map for unidirectional group links.
func BuildTransparentTable(peers []PeerEndpoint, links []GroupLinkPair) *l4.TransparentTable {
	tbl := l4.NewTransparentTable()
	for _, p := range peers {
		if !p.Enabled || p.GroupID == 0 {
			continue
		}
		ip, err := netip.ParseAddr(p.WGIP)
		if err != nil {
			continue
		}
		tbl.RegisterPeer(ip, p.GroupID)
	}
	for _, l := range links {
		if l.Bidirectional {
			continue
		}
		tbl.RegisterUniLink(l.FromGroupID, l.ToGroupID)
	}
	return tbl
}
