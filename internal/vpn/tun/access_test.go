package tun

import (
	"net/netip"
	"testing"
)

func TestRuleSetExclude(t *testing.T) {
	rules := NewRuleSet()
	a := netip.MustParseAddr("100.127.0.2")
	b := netip.MustParseAddr("100.127.0.3")
	c := netip.MustParseAddr("100.127.0.4")

	rules.SetBlocked(a, []netip.Addr{b})

	if !rules.CanAccess(a, c) {
		t.Fatal("unlisted peer should be allowed")
	}
	if rules.CanAccess(a, b) {
		t.Fatal("excluded peer should be blocked")
	}
	if !rules.CanAccess(c, a) {
		t.Fatal("reverse direction should also be symmetrically checked by filter")
	}
}
