package domain

import (
	"net/netip"
	"testing"
)

func TestBuildAccessRules(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 2, ToGroupID: 3}}

	rules, err := BuildAccessRules(peers, links)
	if err != nil {
		t.Fatal(err)
	}

	alice := netip.MustParseAddr("100.127.0.2")
	bob := netip.MustParseAddr("100.127.0.3")
	carol := netip.MustParseAddr("100.127.0.4")

	if rules.CanAccess(alice, bob) {
		t.Fatal("alice should not reach bob without group link")
	}
	if !rules.CanAccess(bob, carol) {
		t.Fatal("bob should reach carol with group link")
	}
	if rules.CanAccess(alice, carol) {
		t.Fatal("alice should not reach carol without group link")
	}
}

func TestGroupsCanAccessSameGroup(t *testing.T) {
	if !GroupsCanAccess(1, 1, nil) {
		t.Fatal("same group should always connect")
	}
}
