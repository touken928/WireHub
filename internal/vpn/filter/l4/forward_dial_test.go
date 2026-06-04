package l4

import (
	"net/netip"
	"testing"
)

func TestChooseOutboundIPv4SkipsVPNSubnet(t *testing.T) {
	subnet, err := parseVPNSubnet("100.127.0.0/24")
	if err != nil {
		t.Fatal(err)
	}
	ip := chooseOutboundIPv4(subnet)
	if ip == nil {
		t.Skip("no outbound interface")
	}
	if addrInSubnet(subnet, netip.AddrFrom4([4]byte(ip.To4()))) {
		t.Fatalf("outbound ip %s must not be in vpn subnet", ip)
	}
}

func TestFirstNonVPNIPv4SkipsUtun(t *testing.T) {
	subnet, err := parseVPNSubnet("100.127.0.0/24")
	if err != nil {
		t.Fatal(err)
	}
	ip := firstNonVPNIPv4(subnet)
	if ip == nil {
		t.Skip("no physical interface")
	}
	if addrInSubnet(subnet, netip.AddrFrom4([4]byte(ip.To4()))) {
		t.Fatalf("physical ip %s must not be in vpn subnet", ip)
	}
}
