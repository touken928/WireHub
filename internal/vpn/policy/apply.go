// Package policy applies portable domain policy specs to the TUN data plane.
package policy

import (
	"github.com/touken928/wirehub/internal/domain/policy"
	"github.com/touken928/wirehub/internal/vpn/snat"
	"github.com/touken928/wirehub/internal/vpn/tun"
)

// Apply builds the runtime TUN access policy from a domain spec.
func Apply(spec policy.AccessPolicySpec) *tun.AccessPolicy {
	rules := tun.NewRuleSet()
	for src, blocked := range spec.Blocked {
		rules.SetBlocked(src, blocked)
	}
	return &tun.AccessPolicy{
		Rules:       rules,
		Transparent: applyTransparent(spec.Transparent),
	}
}

// ApplyRules returns only the block list (for tests that do not need SNAT).
func ApplyRules(spec policy.AccessPolicySpec) *tun.RuleSet {
	return Apply(spec).Rules
}

func applyTransparent(spec policy.TransparentSpec) *snat.TransparentTable {
	tbl := snat.NewTransparentTable()
	for _, p := range spec.Peers {
		if p.GroupID == 0 || !p.WGIP.IsValid() {
			continue
		}
		tbl.RegisterPeer(p.WGIP, p.GroupID)
	}
	for _, l := range spec.UniLinks {
		if l.Bidirectional {
			continue
		}
		tbl.RegisterUniLink(l.FromGroupID, l.ToGroupID)
	}
	return tbl
}

// ApplyTransparent builds only the transparent SNAT table from a spec.
func ApplyTransparent(spec policy.TransparentSpec) *snat.TransparentTable {
	return applyTransparent(spec)
}
