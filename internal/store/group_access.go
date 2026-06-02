package store

import (
	"net/netip"

	"github.com/touken928/wirehub/internal/network"
)

// GroupsCanAccess reports whether two groups may reach each other (undirected link or same group).
func GroupsCanAccess(a, b uint, links []GroupLink) bool {
	if a == 0 || b == 0 {
		return false
	}
	if a == b {
		return true
	}
	for _, l := range links {
		if l.FromGroupID == a && l.ToGroupID == b {
			return true
		}
		if l.FromGroupID == b && l.ToGroupID == a {
			return true
		}
	}
	return false
}

// BuildGroupAccessRules blocks peer-to-peer traffic across groups without an explicit link.
func BuildGroupAccessRules(peers []Peer, links []GroupLink) (*network.AccessRuleSet, error) {
	rules := network.NewAccessRuleSet()
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
			if GroupsCanAccess(p.GroupID, q.GroupID, links) {
				continue
			}
			toIP, err := netip.ParseAddr(q.WGIP)
			if err != nil {
				continue
			}
			blocked = append(blocked, toIP)
		}
		rules.SetBlocked(fromIP, blocked)
	}
	return rules, nil
}
