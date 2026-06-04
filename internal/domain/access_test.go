package domain

import (
	"net/netip"
	"testing"

	"github.com/touken928/wirehub/internal/vpn/filter"
)

func addr(s string) netip.Addr {
	return netip.MustParseAddr(s)
}

func groupPolicy(entries ...GroupAccess) GroupAccessPolicy {
	return NewGroupAccessPolicy(entries)
}

// peerCanSend mirrors TUN filtering: each packet is allowed only when src→dst is permitted.
func peerCanSend(rules *filter.RuleSet, from, to string) bool {
	return rules.CanAccess(addr(from), addr(to))
}

// peersCanReach is true when both peers can send to each other (active↔active or same group).
func peersCanReach(rules *filter.RuleSet, from, to string) bool {
	return peerCanSend(rules, from, to) && peerCanSend(rules, to, from)
}

func TestGroupAccessPolicy_DefaultAllowsIntraGroup(t *testing.T) {
	p := GroupAccessPolicy{}
	if !p.AllowsIntraGroup(10) {
		t.Fatal("missing group id should default to allow intra-group")
	}
	if p.AllowsIntraGroup(0) {
		t.Fatal("group id 0 must not allow intra-group")
	}
}

func TestGroupAccessPolicy_ExplicitDeny(t *testing.T) {
	p := groupPolicy(GroupAccess{ID: 10, AllowIntraGroup: false})
	if p.AllowsIntraGroup(10) {
		t.Fatal("group 10 should deny intra-group")
	}
	if !p.AllowsIntraGroup(20) {
		t.Fatal("other groups should still default to allow")
	}
}

func TestGroupsCanAccess(t *testing.T) {
	links := []GroupLinkPair{{FromGroupID: 2, ToGroupID: 3, Bidirectional: true}}
	grp := GroupAccessPolicy{}

	tests := []struct {
		name string
		a, b uint
		want bool
	}{
		{"same group", 1, 1, true},
		{"different groups no link", 1, 2, false},
		{"linked groups forward order", 2, 3, true},
		{"linked groups reverse order", 3, 2, true},
		{"unlinked groups", 1, 3, false},
		{"zero group id source", 0, 1, false},
		{"zero group id dest", 1, 0, false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if got := GroupsCanAccess(tc.a, tc.b, links, grp); got != tc.want {
				t.Fatalf("GroupsCanAccess(%d, %d) = %v, want %v", tc.a, tc.b, got, tc.want)
			}
		})
	}
}

func TestGroupsCanAccess_SameGroupIntraDisabled(t *testing.T) {
	grp := groupPolicy(GroupAccess{ID: 1, AllowIntraGroup: false})
	if GroupsCanAccess(1, 1, nil, grp) {
		t.Fatal("same group with intra disabled must not connect")
	}
}

func TestBuildAccessRules_SameGroupInterconnect(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 10, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 10, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 10, Enabled: true},
	}
	rules, err := BuildAccessRules(peers, nil, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	pairs := [][2]string{
		{"100.127.0.2", "100.127.0.3"},
		{"100.127.0.2", "100.127.0.4"},
		{"100.127.0.3", "100.127.0.4"},
	}
	for _, p := range pairs {
		if !peersCanReach(rules, p[0], p[1]) {
			t.Fatalf("same-group peers %s and %s should reach each other", p[0], p[1])
		}
	}
}

func TestBuildAccessRules_SameGroupIntraDisabled(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 10, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 10, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 20, Enabled: true},
	}
	grp := groupPolicy(GroupAccess{ID: 10, AllowIntraGroup: false})

	rules, err := BuildAccessRules(peers, nil, grp, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	isolated := [][2]string{
		{"100.127.0.2", "100.127.0.3"},
		{"100.127.0.3", "100.127.0.2"},
	}
	for _, p := range isolated {
		if peersCanReach(rules, p[0], p[1]) {
			t.Fatalf("intra disabled: %s and %s must be isolated", p[0], p[1])
		}
	}
	if peerCanSend(rules, "100.127.0.2", "100.127.0.4") || peerCanSend(rules, "100.127.0.4", "100.127.0.2") {
		t.Fatal("cross-group without link should remain isolated both ways")
	}
}

func TestBuildAccessRules_MixedIntraGroupSettings(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 10, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 10, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 20, Enabled: true},
		{ID: 4, WGIP: "100.127.0.5", GroupID: 20, Enabled: true},
	}
	grp := groupPolicy(
		GroupAccess{ID: 10, AllowIntraGroup: false},
		GroupAccess{ID: 20, AllowIntraGroup: true},
	)
	rules, err := BuildAccessRules(peers, nil, grp, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if peersCanReach(rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("group 10 intra disabled")
	}
	if !peersCanReach(rules, "100.127.0.4", "100.127.0.5") {
		t.Fatal("group 20 intra enabled")
	}
}

func TestBuildAccessRules_CrossGroupIsolation(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	rules, err := BuildAccessRules(peers, nil, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	isolated := [][2]string{
		{"100.127.0.2", "100.127.0.3"},
		{"100.127.0.2", "100.127.0.4"},
		{"100.127.0.3", "100.127.0.4"},
	}
	for _, p := range isolated {
		if peersCanReach(rules, p[0], p[1]) {
			t.Fatalf("groups without links: %s and %s must be isolated", p[0], p[1])
		}
	}
}

func TestBuildAccessRules_LinkedGroupsInterconnect(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 2, ToGroupID: 3, Bidirectional: true}}

	rules, err := BuildAccessRules(peers, links, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if !peersCanReach(rules, "100.127.0.3", "100.127.0.4") {
		t.Fatal("linked groups 2 and 3 should reach each other")
	}
	if peersCanReach(rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("group 1 must stay isolated from group 2 without a link")
	}
	if peersCanReach(rules, "100.127.0.2", "100.127.0.4") {
		t.Fatal("group 1 must stay isolated from group 3 without a link")
	}
}

func TestBuildAccessRules_LinkedGroupsUnaffectedByIntraDisabled(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 1, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 2, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: true}}
	grp := groupPolicy(GroupAccess{ID: 1, AllowIntraGroup: false})

	rules, err := BuildAccessRules(peers, links, grp, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if peersCanReach(rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("intra-group must stay disabled")
	}
	if !peersCanReach(rules, "100.127.0.2", "100.127.0.4") {
		t.Fatal("cross-group bidirectional link must still work")
	}
	if !peersCanReach(rules, "100.127.0.3", "100.127.0.4") {
		t.Fatal("cross-group bidirectional link must still work")
	}
}

func TestBuildAccessRules_LinkIsNotTransitive(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	links := []GroupLinkPair{
		{FromGroupID: 1, ToGroupID: 2, Bidirectional: true},
		{FromGroupID: 2, ToGroupID: 3, Bidirectional: true},
	}

	rules, err := BuildAccessRules(peers, links, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if !peersCanReach(rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("groups 1-2 are linked")
	}
	if !peersCanReach(rules, "100.127.0.3", "100.127.0.4") {
		t.Fatal("groups 2-3 are linked")
	}
	if peersCanReach(rules, "100.127.0.2", "100.127.0.4") {
		t.Fatal("group link is not transitive: 1 and 3 must remain isolated")
	}
}

func TestBuildAccessRules_DisabledPeerSkipped(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: false},
	}
	rules, err := BuildAccessRules(peers, nil, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if !rules.CanAccess(addr("100.127.0.2"), addr("100.127.0.3")) {
		t.Fatal("enabled peer should not block disabled peer destination")
	}
	if !rules.CanAccess(addr("100.127.0.3"), addr("100.127.0.2")) {
		t.Fatal("disabled peer has no outbound block list")
	}
}

func TestBuildAccessRules_NoGroupPeerSkipped(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 0, Enabled: true},
	}
	rules, err := BuildAccessRules(peers, nil, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if !rules.CanAccess(addr("100.127.0.2"), addr("100.127.0.3")) {
		t.Fatal("peer without group should not be blocked by others")
	}
	if _, ok := rules.Rules[addr("100.127.0.3")]; ok {
		t.Fatal("peer without group should not have outbound block rules")
	}
}

func TestBuildAccessRules_MultipleLinks(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
		{ID: 4, WGIP: "100.127.0.5", GroupID: 4, Enabled: true},
	}
	links := []GroupLinkPair{
		{FromGroupID: 1, ToGroupID: 2, Bidirectional: true},
		{FromGroupID: 3, ToGroupID: 4, Bidirectional: true},
	}

	rules, err := BuildAccessRules(peers, links, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	allowed := [][2]string{{"100.127.0.2", "100.127.0.3"}, {"100.127.0.4", "100.127.0.5"}}
	for _, p := range allowed {
		if !peersCanReach(rules, p[0], p[1]) {
			t.Fatalf("linked pair %s <-> %s should communicate", p[0], p[1])
		}
	}
	denied := [][2]string{
		{"100.127.0.2", "100.127.0.4"},
		{"100.127.0.2", "100.127.0.5"},
		{"100.127.0.3", "100.127.0.4"},
	}
	for _, p := range denied {
		if peersCanReach(rules, p[0], p[1]) {
			t.Fatalf("unlinked pair %s <-> %s must be isolated", p[0], p[1])
		}
	}
}

func TestBuildAccessRules(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 2, ToGroupID: 3, Bidirectional: true}}

	rules, err := BuildAccessRules(peers, links, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if peersCanReach(rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("alice should not reach bob without group link")
	}
	if !peersCanReach(rules, "100.127.0.3", "100.127.0.4") {
		t.Fatal("bob should reach carol with group link")
	}
	if peersCanReach(rules, "100.127.0.2", "100.127.0.4") {
		t.Fatal("alice should not reach carol without group link")
	}
}

func TestGroupsCanAccessSameGroup(t *testing.T) {
	if !GroupsCanAccess(1, 1, nil, GroupAccessPolicy{}) {
		t.Fatal("same group should connect by default")
	}
}

func TestLinkAllowsInit_Unidirectional(t *testing.T) {
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}
	grp := GroupAccessPolicy{}

	if !LinkAllowsInit(1, 2, links, grp) {
		t.Fatal("source group should reach target")
	}
	if LinkAllowsInit(2, 1, links, grp) {
		t.Fatal("reverse direction must be blocked")
	}
}

func TestLinkAllowsInit_SameGroupIntraDisabled(t *testing.T) {
	grp := groupPolicy(GroupAccess{ID: 5, AllowIntraGroup: false})
	if LinkAllowsInit(5, 5, nil, grp) {
		t.Fatal("same group with intra disabled must not allow init")
	}
}

func TestBuildAccessRules_UnidirectionalSNATPath(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, Name: "client", WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, Name: "svc", WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}

	policy, err := BuildAccessPolicy(peers, links, GroupAccessPolicy{}, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}
	if !peerCanSend(policy.Rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("client should not be blocked toward service (SNAT applies on hub TUN)")
	}
	if peerCanSend(policy.Rules, "100.127.0.3", "100.127.0.2") {
		t.Fatal("service must not reach client")
	}
}

func TestBuildAccessRules_MapGroupACL(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 10, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 20, Enabled: true},
	}
	maps := NewMapAccessPolicy([]MapAccess{{
		VirtualIP:       "100.127.0.50",
		AllowedGroupIDs: map[uint]struct{}{10: {}},
	}})
	rules, err := BuildAccessRules(peers, nil, GroupAccessPolicy{}, maps)
	if err != nil {
		t.Fatal(err)
	}
	if !peerCanSend(rules, "100.127.0.2", "100.127.0.50") {
		t.Fatal("group 10 should reach map vip")
	}
	if peerCanSend(rules, "100.127.0.3", "100.127.0.50") {
		t.Fatal("group 20 must not reach map vip")
	}
}

func TestBuildAccessRules_UnidirectionalSNAT_IntraDisabledDoesNotBlockCrossGroup(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 1, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 2, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}
	grp := groupPolicy(GroupAccess{ID: 1, AllowIntraGroup: false})

	policy, err := BuildAccessPolicy(peers, links, grp, MapAccessPolicy{})
	if err != nil {
		t.Fatal(err)
	}

	if peerCanSend(policy.Rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("same group intra disabled")
	}
	if !peerCanSend(policy.Rules, "100.127.0.2", "100.127.0.4") {
		t.Fatal("uni link SNAT path must remain open")
	}
	if peerCanSend(policy.Rules, "100.127.0.4", "100.127.0.2") {
		t.Fatal("reverse uni direction blocked")
	}
}
