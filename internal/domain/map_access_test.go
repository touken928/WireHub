package domain

import "testing"

func TestAllowedGroupIDSet(t *testing.T) {
	set := AllowedGroupIDSet([]uint{1, 2, 1, 0})
	if len(set) != 2 {
		t.Fatalf("set len = %d", len(set))
	}
	if !GroupInAllowedSet(set, 1) || !GroupInAllowedSet(set, 2) {
		t.Fatal("expected groups 1 and 2")
	}
	if GroupInAllowedSet(set, 3) || GroupInAllowedSet(nil, 1) {
		t.Fatal("expected deny")
	}
}

func TestNewMapAccess(t *testing.T) {
	r := NewMapAccess("100.127.0.50", []uint{10})
	if r.VirtualIP != "100.127.0.50" || !GroupInAllowedSet(r.AllowedGroupIDs, 10) {
		t.Fatal(r)
	}
}
