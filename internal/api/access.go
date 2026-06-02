package api

import (
	"github.com/touken928/wirehub/internal/network"
	"github.com/touken928/wirehub/internal/store"
)

func buildAccessRules(st *store.Store) (*network.AccessRuleSet, error) {
	peers, err := st.ListPeers()
	if err != nil {
		return nil, err
	}
	links, err := st.ListGroupLinks()
	if err != nil {
		return nil, err
	}
	return store.BuildGroupAccessRules(peers, links)
}

func (s *Server) SyncAccessFilter() {
	rules, err := buildAccessRules(s.store)
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
