package domain

import "testing"

func TestLinkAllowsInit_UnidirectionalMatchesStoredDirection(t *testing.T) {
	links := []GroupLinkPair{{FromGroupID: 10, ToGroupID: 20, Bidirectional: false}}

	if !LinkAllowsInit(10, 20, links) {
		t.Fatal("from_group_id → to_group_id must allow init")
	}
	if LinkAllowsInit(20, 10, links) {
		t.Fatal("reverse direction must not allow init")
	}
}
