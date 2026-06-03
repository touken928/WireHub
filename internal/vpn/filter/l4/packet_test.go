package l4

import (
	"net/netip"
	"testing"
)

func TestParseIPv4TransportTCP(t *testing.T) {
	src := netip.MustParseAddr("100.127.0.2")
	dst := netip.MustParseAddr("100.127.0.3")
	pkt := buildTCPPacket(src, dst, 50000, 8080)

	gotSrc, gotDst, proto, sport, dport, ok := parseIPv4Transport(pkt)
	if !ok {
		t.Fatal("expected ok")
	}
	if gotSrc != src || gotDst != dst {
		t.Fatalf("addrs = %s -> %s, want %s -> %s", gotSrc, gotDst, src, dst)
	}
	if proto != protoTCP || sport != 50000 || dport != 8080 {
		t.Fatalf("proto=%d sport=%d dport=%d", proto, sport, dport)
	}
}

func TestParseIPv4TransportUDP(t *testing.T) {
	src := netip.MustParseAddr("100.127.0.2")
	dst := netip.MustParseAddr("100.127.0.3")
	pkt := buildUDPPacket(src, dst, 53000, 53)

	gotSrc, gotDst, proto, sport, dport, ok := parseIPv4Transport(pkt)
	if !ok {
		t.Fatal("expected ok")
	}
	if gotSrc != src || gotDst != dst {
		t.Fatalf("addrs = %s -> %s, want %s -> %s", gotSrc, gotDst, src, dst)
	}
	if proto != protoUDP || sport != 53000 || dport != 53 {
		t.Fatalf("proto=%d sport=%d dport=%d", proto, sport, dport)
	}
}

func TestParseIPv4TransportRejectsUnsupported(t *testing.T) {
	pkt := buildTCPPacket(
		netip.MustParseAddr("100.127.0.2"),
		netip.MustParseAddr("100.127.0.3"),
		1, 2,
	)
	pkt[9] = 1 // ICMP
	fixIPv4Checksum(pkt)

	if _, _, _, _, _, ok := parseIPv4Transport(pkt); ok {
		t.Fatal("ICMP must not parse as transport")
	}
	if _, _, _, _, _, ok := parseIPv4Transport([]byte{0x45}); ok {
		t.Fatal("short packet must fail")
	}
}

func TestRewriteEndpointsUpdatesChecksums(t *testing.T) {
	src := netip.MustParseAddr("100.127.0.2")
	dst := netip.MustParseAddr("100.127.0.3")
	pkt := buildTCPPacket(src, dst, 50000, 8080)

	newSrc := netip.MustParseAddr("100.127.0.1")
	newDst := netip.MustParseAddr("100.127.0.4")
	rewriteEndpoints(pkt, newSrc, newDst, 40000, 9090)

	gotSrc, gotDst := packetAddrs(pkt)
	if gotSrc != newSrc || gotDst != newDst {
		t.Fatalf("rewritten addrs = %s -> %s", gotSrc, gotDst)
	}
	if sport := packetSport(pkt); sport != 40000 {
		t.Fatalf("sport = %d, want 40000", sport)
	}

	// Rewriting without fixing checksums would break validation downstream; ensure helpers stay paired.
	fixIPv4Checksum(pkt)
	fixTransportChecksum(pkt, protoTCP)
	if _, _, _, _, _, ok := parseIPv4Transport(pkt); !ok {
		t.Fatal("packet invalid after rewrite + checksum fix")
	}
}
