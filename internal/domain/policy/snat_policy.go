package policy

import "net/netip"

// BuildTransparentSpec collects peer groups and unidirectional links for hub TUN SNAT.
func BuildTransparentSpec(peers []PeerEndpoint, links []GroupLinkPair) TransparentSpec {
	var spec TransparentSpec
	for _, p := range peers {
		if !p.Enabled || p.GroupID == 0 {
			continue
		}
		ip, err := netip.ParseAddr(p.WGIP)
		if err != nil {
			continue
		}
		spec.Peers = append(spec.Peers, TransparentPeer{WGIP: ip, GroupID: p.GroupID})
	}
	for _, l := range links {
		if l.Bidirectional {
			continue
		}
		spec.UniLinks = append(spec.UniLinks, l)
	}
	return spec
}
