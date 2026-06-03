package l4

import (
	"net/netip"
	"testing"
)

func TestTransparentRelayTCPRoundTrip(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	client := netip.MustParseAddr("100.127.0.2")
	server := netip.MustParseAddr("100.127.0.3")

	tbl := newRelayTable(t, hub, map[netip.Addr]uint{
		client: 1,
		server: 2,
	}, [][2]uint{{1, 2}})

	out := buildTCPPacket(client, server, 50000, 8080)
	if !tbl.ProcessEgressToWG(out) {
		t.Fatal("expected outbound SNAT")
	}
	src, _ := packetAddrs(out)
	if src != hub {
		t.Fatalf("src after SNAT = %s, want %s", src, hub)
	}
	ephemeral := packetSport(out)
	if ephemeral < EphemeralPortMin {
		t.Fatalf("ephemeral port = %d", ephemeral)
	}

	ret := buildTCPPacket(server, hub, 8080, ephemeral)
	if !tbl.ProcessIngressFromWG(ret) {
		t.Fatal("expected return DNAT")
	}
	_, dst := packetAddrs(ret)
	if dst != client {
		t.Fatalf("dst after return = %s, want %s", dst, client)
	}
}

func TestTransparentRelayUDPRoundTrip(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	client := netip.MustParseAddr("100.127.0.2")
	server := netip.MustParseAddr("100.127.0.3")

	tbl := newRelayTable(t, hub, map[netip.Addr]uint{
		client: 10,
		server: 20,
	}, [][2]uint{{10, 20}})

	out := buildUDPPacket(client, server, 54000, 5353)
	if !tbl.ProcessEgressToWG(out) {
		t.Fatal("expected outbound UDP SNAT")
	}
	if src, _ := packetAddrs(out); src != hub {
		t.Fatalf("src after SNAT = %s, want %s", src, hub)
	}
	ephemeral := packetSport(out)

	ret := buildUDPPacket(server, hub, 5353, ephemeral)
	if !tbl.ProcessIngressFromWG(ret) {
		t.Fatal("expected return UDP DNAT")
	}
	if _, dst := packetAddrs(ret); dst != client {
		t.Fatalf("dst after return = %s, want %s", dst, client)
	}
}

func TestTransparentRelaySkipsWithoutUniLink(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	client := netip.MustParseAddr("100.127.0.2")
	server := netip.MustParseAddr("100.127.0.3")

	tbl := newRelayTable(t, hub, map[netip.Addr]uint{
		client: 1,
		server: 2,
	}, nil)

	pkt := buildTCPPacket(client, server, 50000, 8080)
	if tbl.ProcessEgressToWG(pkt) {
		t.Fatal("bidirectional or unlinked traffic must not SNAT")
	}
}

func TestTransparentRelaySkipsReverseDirection(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	client := netip.MustParseAddr("100.127.0.2")
	server := netip.MustParseAddr("100.127.0.3")

	tbl := newRelayTable(t, hub, map[netip.Addr]uint{
		client: 1,
		server: 2,
	}, [][2]uint{{1, 2}})

	pkt := buildTCPPacket(server, client, 8080, 50000)
	if tbl.ProcessEgressToWG(pkt) {
		t.Fatal("reverse direction must not SNAT on egress")
	}
}

func TestTransparentRelaySkipsSameGroup(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	a := netip.MustParseAddr("100.127.0.2")
	b := netip.MustParseAddr("100.127.0.3")

	tbl := newRelayTable(t, hub, map[netip.Addr]uint{
		a: 1,
		b: 1,
	}, [][2]uint{{1, 2}})

	pkt := buildTCPPacket(a, b, 50000, 8080)
	if tbl.ProcessEgressToWG(pkt) {
		t.Fatal("same-group traffic must not SNAT")
	}
}

func TestTransparentRelayReusesSessionPort(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	client := netip.MustParseAddr("100.127.0.2")
	server := netip.MustParseAddr("100.127.0.3")

	tbl := newRelayTable(t, hub, map[netip.Addr]uint{
		client: 1,
		server: 2,
	}, [][2]uint{{1, 2}})

	first := buildTCPPacket(client, server, 50000, 8080)
	if !tbl.ProcessEgressToWG(first) {
		t.Fatal("expected SNAT")
	}
	port1 := packetSport(first)

	second := buildTCPPacket(client, server, 50000, 8080)
	if !tbl.ProcessEgressToWG(second) {
		t.Fatal("expected SNAT on same flow")
	}
	if port2 := packetSport(second); port2 != port1 {
		t.Fatalf("session port changed: %d then %d", port1, port2)
	}
}

func TestTransparentRelaySkipsReservedHubPorts(t *testing.T) {
	tbl := NewTransparentTable()
	tbl.SetHubIP(netip.MustParseAddr("100.127.0.1"))
	tbl.ReserveHubPorts([]int{25000})

	for i := 0; i < 512; i++ {
		port, err := tbl.ports.pick()
		if err != nil {
			t.Fatal(err)
		}
		if port == 25000 {
			t.Fatal("picked reserved hub listen port")
		}
	}
}

func TestTransparentRelayIgnoresIngressWithoutSession(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	tbl := NewTransparentTable()
	tbl.SetHubIP(hub)

	pkt := buildTCPPacket(
		netip.MustParseAddr("100.127.0.3"),
		hub,
		8080,
		45000,
	)
	if tbl.ProcessIngressFromWG(pkt) {
		t.Fatal("unknown hub port must not rewrite ingress")
	}
}
