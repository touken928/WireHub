package domain

import (
	"net/netip"
	"testing"

	"github.com/touken928/wirehub/internal/vpn/filter"
)

func addr(s string) netip.Addr {
	return netip.MustParseAddr(s)
}

// peerCanSend mirrors TUN filtering: each packet is allowed only when src→dst is permitted.
func peerCanSend(rules *filter.RuleSet, from, to string) bool {
	return rules.CanAccess(addr(from), addr(to))
}

// peersCanReach is true when both peers can send to each other (active↔active or same group).
func peersCanReach(rules *filter.RuleSet, from, to string) bool {
	return peerCanSend(rules, from, to) && peerCanSend(rules, to, from)
}

func TestGroupsCanAccess(t *testing.T) {
	links := []GroupLinkPair{{FromGroupID: 2, ToGroupID: 3, Bidirectional: true}}

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
			if got := GroupsCanAccess(tc.a, tc.b, links); got != tc.want {
				t.Fatalf("GroupsCanAccess(%d, %d) = %v, want %v", tc.a, tc.b, got, tc.want)
			}
		})
	}
}

func TestBuildAccessRules_SameGroupInterconnect(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 10, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 10, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 10, Enabled: true},
	}
	rules, err := BuildAccessRules(peers, nil)
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

func TestBuildAccessRules_CrossGroupIsolation(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	rules, err := BuildAccessRules(peers, nil)
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

	rules, err := BuildAccessRules(peers, links)
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

	rules, err := BuildAccessRules(peers, links)
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
	rules, err := BuildAccessRules(peers, nil)
	if err != nil {
		t.Fatal(err)
	}

	// Active peer does not block toward disabled peer (disabled peer is omitted from policy).
	if !rules.CanAccess(addr("100.127.0.2"), addr("100.127.0.3")) {
		t.Fatal("enabled peer should not block disabled peer destination")
	}
	// Disabled peer has no rule entry; filter treats missing rule as allow on that side.
	if !rules.CanAccess(addr("100.127.0.3"), addr("100.127.0.2")) {
		t.Fatal("disabled peer has no outbound block list")
	}
}

func TestBuildAccessRules_NoGroupPeerSkipped(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 0, Enabled: true},
	}
	rules, err := BuildAccessRules(peers, nil)
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

	rules, err := BuildAccessRules(peers, links)
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

// TestBuildAccessRules is the original scenario: groups 2↔3 linked, group 1 isolated.
func TestBuildAccessRules(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{ID: 3, WGIP: "100.127.0.4", GroupID: 3, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 2, ToGroupID: 3, Bidirectional: true}}

	rules, err := BuildAccessRules(peers, links)
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
	if !GroupsCanAccess(1, 1, nil) {
		t.Fatal("same group should always connect")
	}
}

func TestLinkAllowsInit_Unidirectional(t *testing.T) {
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}

	if !LinkAllowsInit(1, 2, links) {
		t.Fatal("source group should reach target")
	}
	if LinkAllowsInit(2, 1, links) {
		t.Fatal("reverse direction must be blocked")
	}
}

func TestBuildAccessRules_UnidirectionalSNATPath(t *testing.T) {
	peers := []PeerEndpoint{
		{ID: 1, Name: "client", WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{ID: 2, Name: "svc", WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}

	policy, err := BuildAccessPolicy(peers, links)
	if err != nil {
		t.Fatal(err)
	}
	// Raw WG IP path is not blocked; hub SNAT handles client → service.
	if !peerCanSend(policy.Rules, "100.127.0.2", "100.127.0.3") {
		t.Fatal("client should not be blocked toward service (SNAT applies on hub TUN)")
	}
	if peerCanSend(policy.Rules, "100.127.0.3", "100.127.0.2") {
		t.Fatal("service must not reach client")
	}
}

