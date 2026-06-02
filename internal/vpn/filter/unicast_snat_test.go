package filter

import (
	"encoding/binary"
	"net/netip"
	"testing"
)

func buildTCPPacket(src, dst netip.Addr, sport, dport uint16) []byte {
	p := make([]byte, 40)
	p[0] = 0x45
	p[1] = 0
	binary.BigEndian.PutUint16(p[2:4], uint16(len(p)))
	p[9] = protoTCP
	s4 := src.As4()
	d4 := dst.As4()
	copy(p[12:16], s4[:])
	copy(p[16:20], d4[:])
	binary.BigEndian.PutUint16(p[20:22], sport)
	binary.BigEndian.PutUint16(p[22:24], dport)
	fixIPv4Checksum(p)
	fixTransportChecksum(p, protoTCP)
	return p
}

func TestUniSNATOutboundAndReturn(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	client := netip.MustParseAddr("100.127.0.2")
	server := netip.MustParseAddr("100.127.0.3")

	tbl := NewUniSNATTable()
	tbl.SetHubIP(hub)
	tbl.RegisterPeer(client, 1)
	tbl.RegisterPeer(server, 2)
	tbl.RegisterUniLink(1, 2)

	out := buildTCPPacket(client, server, 50000, 8080)
	if !tbl.ProcessEgressToWG(out) {
		t.Fatal("expected outbound SNAT")
	}
	if got := netip.AddrFrom4([4]byte{out[12], out[13], out[14], out[15]}); got != hub {
		t.Fatalf("src after SNAT = %s, want %s", got, hub)
	}
	ephemeral := binary.BigEndian.Uint16(out[20:22])
	if ephemeral < uniSNATPortMin {
		t.Fatalf("ephemeral port = %d", ephemeral)
	}

	ret := buildTCPPacket(server, hub, 8080, ephemeral)
	if !tbl.ProcessIngressFromWG(ret) {
		t.Fatal("expected return SNAT")
	}
	if got := netip.AddrFrom4([4]byte{ret[16], ret[17], ret[18], ret[19]}); got != client {
		t.Fatalf("dst after return = %s, want %s", got, client)
	}
}
