package domain

import (
	"net/netip"

	"github.com/touken928/wirehub/internal/vpn/filter"
)

// PeerEndpoint is the minimum peer data required for group access policy.
type PeerEndpoint struct {
	ID      uint
	Name    string
	DNSName string
	WGIP    string
	GroupID uint
	Enabled bool
}

// GroupLinkPair is a directed or bidirectional link between two peer groups.
type GroupLinkPair struct {
	FromGroupID   uint
	ToGroupID     uint
	Bidirectional bool // true: both directions; false: FromGroupID → ToGroupID only
}

func findLinkBetween(a, b uint, links []GroupLinkPair) *GroupLinkPair {
	for i := range links {
		l := &links[i]
		if l.Bidirectional {
			if (l.FromGroupID == a && l.ToGroupID == b) || (l.FromGroupID == b && l.ToGroupID == a) {
				return l
			}
			continue
		}
		if l.FromGroupID == a && l.ToGroupID == b {
			return l
		}
		if l.FromGroupID == b && l.ToGroupID == a {
			return l
		}
	}
	return nil
}

// LinkAllowsInit reports whether traffic may flow from fromGroup to toGroup (policy).
func LinkAllowsInit(fromGroup, toGroup uint, links []GroupLinkPair) bool {
	if fromGroup == 0 || toGroup == 0 {
		return false
	}
	if fromGroup == toGroup {
		return true
	}
	for _, l := range links {
		if l.Bidirectional {
			if (l.FromGroupID == fromGroup && l.ToGroupID == toGroup) ||
				(l.FromGroupID == toGroup && l.ToGroupID == fromGroup) {
				return true
			}
			continue
		}
		if l.FromGroupID == fromGroup && l.ToGroupID == toGroup {
			return true
		}
	}
	return false
}

// GroupsCanAccess is true when any access (either direction) is allowed between groups.
func GroupsCanAccess(a, b uint, links []GroupLinkPair) bool {
	return LinkAllowsInit(a, b, links) || LinkAllowsInit(b, a, links)
}

// allowDirectPeerIP is true for same-group or bidirectional cross-group (direct WG IP).
func allowDirectPeerIP(p, q PeerEndpoint, links []GroupLinkPair) bool {
	if p.GroupID == 0 || q.GroupID == 0 {
		return false
	}
	if p.GroupID == q.GroupID {
		return true
	}
	l := findLinkBetween(p.GroupID, q.GroupID, links)
	if l == nil {
		return false
	}
	return l.Bidirectional
}

// needsUniSNAT is true for unidirectional cross-group traffic (hub SNAT, ephemeral ports).
func needsUniSNAT(p, q PeerEndpoint, links []GroupLinkPair) bool {
	if p.GroupID == 0 || q.GroupID == 0 || p.GroupID == q.GroupID {
		return false
	}
	if !LinkAllowsInit(p.GroupID, q.GroupID, links) {
		return false
	}
	return !allowDirectPeerIP(p, q, links)
}

// BuildAccessPolicy configures ACL blocking and hub SNAT for unidirectional group links.
func BuildAccessPolicy(peers []PeerEndpoint, links []GroupLinkPair) (*filter.AccessPolicy, error) {
	rules := filter.NewRuleSet()
	snat := filter.NewUniSNATTable()

	peerByIP := make(map[netip.Addr]PeerEndpoint, len(peers))
	for _, p := range peers {
		if !p.Enabled || p.GroupID == 0 {
			continue
		}
		ip, err := netip.ParseAddr(p.WGIP)
		if err != nil {
			continue
		}
		peerByIP[ip] = p
		snat.RegisterPeer(ip, p.GroupID)
	}
	for _, l := range links {
		if l.Bidirectional {
			continue
		}
		snat.RegisterUniLink(l.FromGroupID, l.ToGroupID)
	}

	for _, p := range peers {
		if !p.Enabled || p.GroupID == 0 {
			continue
		}
		fromIP, err := netip.ParseAddr(p.WGIP)
		if err != nil {
			continue
		}
		blocked := make([]netip.Addr, 0)
		for _, q := range peers {
			if !q.Enabled || q.ID == p.ID || q.GroupID == 0 {
				continue
			}
			toIP, err := netip.ParseAddr(q.WGIP)
			if err != nil {
				continue
			}
			if allowDirectPeerIP(p, q, links) {
				continue
			}
			if needsUniSNAT(p, q, links) {
				continue // hub SNAT path handles allowed A→B
			}
			if LinkAllowsInit(p.GroupID, q.GroupID, links) {
				continue
			}
			blocked = append(blocked, toIP)
		}
		rules.SetBlocked(fromIP, blocked)
	}
	return &filter.AccessPolicy{Rules: rules, SNAT: snat}, nil
}

// BuildAccessRules is kept for tests that only need the block list.
func BuildAccessRules(peers []PeerEndpoint, links []GroupLinkPair) (*filter.RuleSet, error) {
	p, err := BuildAccessPolicy(peers, links)
	if err != nil {
		return nil, err
	}
	return p.Rules, nil
}
