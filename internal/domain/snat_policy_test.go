package domain

import (
	"encoding/binary"
	"net/netip"
	"testing"
)

func buildTestTCPPacket(src, dst netip.Addr, sport, dport uint16) []byte {
	p := make([]byte, 40)
	p[0] = 0x45
	binary.BigEndian.PutUint16(p[2:4], uint16(len(p)))
	p[9] = 6
	s4 := src.As4()
	d4 := dst.As4()
	copy(p[12:16], s4[:])
	copy(p[16:20], d4[:])
	binary.BigEndian.PutUint16(p[20:22], sport)
	binary.BigEndian.PutUint16(p[22:24], dport)
	return p
}

func transparentRelayApplies(tbl interface {
	SetHubIP(netip.Addr)
	ProcessEgressToWG([]byte) bool
}, hub, client, server string) bool {
	tbl.SetHubIP(netip.MustParseAddr(hub))
	pkt := buildTestTCPPacket(
		netip.MustParseAddr(client),
		netip.MustParseAddr(server),
		50000,
		8080,
	)
	return tbl.ProcessEgressToWG(pkt)
}

func TestBuildTransparentTable_UniLinkEnablesRelay(t *testing.T) {
	peers := []PeerEndpoint{
		{WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}

	tbl := BuildTransparentTable(peers, links)
	if !transparentRelayApplies(tbl, "100.127.0.1", "100.127.0.2", "100.127.0.3") {
		t.Fatal("uni link 1→2 must enable transparent map")
	}
	if transparentRelayApplies(tbl, "100.127.0.1", "100.127.0.3", "100.127.0.2") {
		t.Fatal("reverse direction must not map")
	}
}

func TestBuildTransparentTable_SkipsBidirectionalLinks(t *testing.T) {
	peers := []PeerEndpoint{
		{WGIP: "100.127.0.2", GroupID: 1, Enabled: true},
		{WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: true}}

	tbl := BuildTransparentTable(peers, links)
	if transparentRelayApplies(tbl, "100.127.0.1", "100.127.0.2", "100.127.0.3") {
		t.Fatal("bidirectional links use direct WG IP, not transparent map")
	}
}

func TestBuildTransparentTable_SkipsDisabledAndNoGroupPeers(t *testing.T) {
	peers := []PeerEndpoint{
		{WGIP: "100.127.0.2", GroupID: 1, Enabled: false},
		{WGIP: "100.127.0.3", GroupID: 2, Enabled: true},
		{WGIP: "100.127.0.4", GroupID: 0, Enabled: true},
	}
	links := []GroupLinkPair{{FromGroupID: 1, ToGroupID: 2, Bidirectional: false}}

	tbl := BuildTransparentTable(peers, links)
	if transparentRelayApplies(tbl, "100.127.0.1", "100.127.0.3", "100.127.0.4") {
		t.Fatal("disabled / no-group peers must not participate in map table")
	}
}
