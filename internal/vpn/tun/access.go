package tun

import "net/netip"

type peerRule struct {
	Blocked map[netip.Addr]struct{}
}

// RuleSet holds per-peer blocked destination IPs for group ACL enforcement.
type RuleSet struct {
	Rules map[netip.Addr]peerRule
}

func NewRuleSet() *RuleSet {
	return &RuleSet{Rules: make(map[netip.Addr]peerRule)}
}

func (s *RuleSet) SetBlocked(peerIP netip.Addr, blockedIPs []netip.Addr) {
	if len(blockedIPs) == 0 {
		return
	}
	rule := peerRule{Blocked: make(map[netip.Addr]struct{}, len(blockedIPs))}
	for _, ip := range blockedIPs {
		rule.Blocked[ip] = struct{}{}
	}
	s.Rules[peerIP] = rule
}

func (s *RuleSet) CanAccess(from, to netip.Addr) bool {
	if !from.IsValid() || !to.IsValid() {
		return true
	}
	rule, ok := s.Rules[from]
	if !ok || len(rule.Blocked) == 0 {
		return true
	}
	_, blocked := rule.Blocked[to]
	return !blocked
}
