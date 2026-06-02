package api

import (
	"net/netip"

	"github.com/touken928/wirehub/internal/network"
	"github.com/touken928/wirehub/internal/store"
)

func buildAccessRules(peers []store.Peer) (*network.AccessRuleSet, error) {
	rules := network.NewAccessRuleSet()
	for _, p := range peers {
		if !p.Enabled {
			continue
		}
		peerIP, err := netip.ParseAddr(p.WGIP)
		if err != nil {
			continue
		}
		if len(p.AccessExclude) == 0 {
			continue
		}
		blocked, err := store.ResolveExcludeRules(peers, p, p.AccessExclude)
		if err != nil {
			return nil, err
		}
		targets := make([]netip.Addr, 0, len(blocked))
		for _, ipStr := range blocked {
			if ip, err := netip.ParseAddr(ipStr); err == nil {
				targets = append(targets, ip)
			}
		}
		rules.SetBlocked(peerIP, targets)
	}
	return rules, nil
}

func (s *Server) SyncAccessFilter() {
	peers, err := s.store.ListPeers()
	if err != nil {
		return
	}
	rules, err := buildAccessRules(peers)
	if err != nil {
		return
	}
	wgMgr, err := s.wgMgr()
	if err != nil {
		return
	}
	wgMgr.SetAccessRules(rules)
}

func (s *Server) syncAccessFilter() {
	s.SyncAccessFilter()
}
