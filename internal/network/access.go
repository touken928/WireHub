package network

import "net/netip"

type PeerAccessRule struct {
	Blocked map[netip.Addr]struct{}
}

type AccessRuleSet struct {
	Rules map[netip.Addr]PeerAccessRule
}

func NewAccessRuleSet() *AccessRuleSet {
	return &AccessRuleSet{Rules: make(map[netip.Addr]PeerAccessRule)}
}

func (s *AccessRuleSet) SetBlocked(peerIP netip.Addr, blockedIPs []netip.Addr) {
	if len(blockedIPs) == 0 {
		return
	}
	rule := PeerAccessRule{Blocked: make(map[netip.Addr]struct{}, len(blockedIPs))}
	for _, ip := range blockedIPs {
		rule.Blocked[ip] = struct{}{}
	}
	s.Rules[peerIP] = rule
}

func (s *AccessRuleSet) CanAccess(from, to netip.Addr) bool {
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
