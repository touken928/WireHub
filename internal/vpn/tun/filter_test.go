package tun

import (
	"net/netip"
	"testing"
)

func TestShouldDrop_MapVIPBypassesPeerACL(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	vip := netip.MustParseAddr("100.127.0.2")
	peer := netip.MustParseAddr("100.127.0.5")

	f := NewTUN(nil, hub)
	rules := NewRuleSet()
	rules.SetBlocked(peer, []netip.Addr{vip})
	f.SetAccessPolicy(&AccessPolicy{Rules: rules})
	f.SetMapVIPs([]netip.Addr{vip})

	pkt := ipv4Packet(peer, vip)
	if f.shouldDrop(pkt) {
		t.Fatal("traffic to map VIP must reach MapProxy (group ACL is enforced there, like hub forwards)")
	}
}

func ipv4Packet(src, dst netip.Addr) []byte {
	b := make([]byte, 20)
	b[0] = 0x45
	s := src.As4()
	d := dst.As4()
	copy(b[12:16], s[:])
	copy(b[16:20], d[:])
	return b
}
