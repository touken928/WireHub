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

// GroupAccess describes per-group ACL options used when building access policy.
type GroupAccess struct {
	ID              uint
	AllowIntraGroup bool
}

// GroupAccessPolicy holds group-level ACL settings. Missing group IDs default to allow intra-group traffic.
type GroupAccessPolicy struct {
	byID map[uint]bool
}

// NewGroupAccessPolicy builds a lookup table from repo groups.
func NewGroupAccessPolicy(groups []GroupAccess) GroupAccessPolicy {
	byID := make(map[uint]bool, len(groups))
	for _, g := range groups {
		byID[g.ID] = g.AllowIntraGroup
	}
	return GroupAccessPolicy{byID: byID}
}

// AllowsIntraGroup reports whether peers in the same group may reach each other directly.
func (p GroupAccessPolicy) AllowsIntraGroup(groupID uint) bool {
	if groupID == 0 {
		return false
	}
	if p.byID == nil {
		return true
	}
	if allow, ok := p.byID[groupID]; ok {
		return allow
	}
	return true
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
func LinkAllowsInit(fromGroup, toGroup uint, links []GroupLinkPair, groups GroupAccessPolicy) bool {
	if fromGroup == 0 || toGroup == 0 {
		return false
	}
	if fromGroup == toGroup {
		return groups.AllowsIntraGroup(fromGroup)
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
func GroupsCanAccess(a, b uint, links []GroupLinkPair, groups GroupAccessPolicy) bool {
	return LinkAllowsInit(a, b, links, groups) || LinkAllowsInit(b, a, links, groups)
}

// allowDirectPeerIP is true for same-group (when allowed) or bidirectional cross-group (direct WG IP).
func allowDirectPeerIP(p, q PeerEndpoint, links []GroupLinkPair, groups GroupAccessPolicy) bool {
	if p.GroupID == 0 || q.GroupID == 0 {
		return false
	}
	if p.GroupID == q.GroupID {
		return groups.AllowsIntraGroup(p.GroupID)
	}
	l := findLinkBetween(p.GroupID, q.GroupID, links)
	if l == nil {
		return false
	}
	return l.Bidirectional
}

// needsTransparentRelay is true for unidirectional cross-group traffic (hub TUN SNAT).
func needsTransparentRelay(p, q PeerEndpoint, links []GroupLinkPair, groups GroupAccessPolicy) bool {
	if p.GroupID == 0 || q.GroupID == 0 || p.GroupID == q.GroupID {
		return false
	}
	if !LinkAllowsInit(p.GroupID, q.GroupID, links, groups) {
		return false
	}
	return !allowDirectPeerIP(p, q, links, groups)
}

// BuildAccessPolicy configures ACL blocking and hub SNAT for unidirectional group links.
func BuildAccessPolicy(peers []PeerEndpoint, links []GroupLinkPair, groups GroupAccessPolicy) (*filter.AccessPolicy, error) {
	rules := filter.NewRuleSet()
	transparent := BuildTransparentTable(peers, links)

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
			if allowDirectPeerIP(p, q, links, groups) {
				continue
			}
			if needsTransparentRelay(p, q, links, groups) {
				continue // transparent relay handles allowed A→B
			}
			if LinkAllowsInit(p.GroupID, q.GroupID, links, groups) {
				continue
			}
			blocked = append(blocked, toIP)
		}
		rules.SetBlocked(fromIP, blocked)
	}
	return &filter.AccessPolicy{Rules: rules, Transparent: transparent}, nil
}

// BuildAccessRules is kept for tests that only need the block list.
func BuildAccessRules(peers []PeerEndpoint, links []GroupLinkPair, groups GroupAccessPolicy) (*filter.RuleSet, error) {
	p, err := BuildAccessPolicy(peers, links, groups)
	if err != nil {
		return nil, err
	}
	return p.Rules, nil
}
