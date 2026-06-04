package repo

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
)

func testIPAllocStore(t *testing.T) *Store {
	t.Helper()
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	settings := &Settings{
		WGSubnet: "100.127.0.0/24",
		HubIP:    "100.127.0.1",
		DNSIP:    "100.127.0.1",
	}
	if err := st.db.Create(settings).Error; err != nil {
		t.Fatal(err)
	}
	return st
}

func TestIPAllocation_MapThenPeer_NoOverlap(t *testing.T) {
	st := testIPAllocStore(t)
	g, err := st.CreateGroup("users", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	mapDetail, err := st.CreateServiceMap(MapInput{
		Slug:          "svc-a",
		TargetHost:    "127.0.0.1",
		AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}

	peerIP, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if peerIP == mapDetail.VirtualIP {
		t.Fatalf("peer IP %q collides with map VIP %q", peerIP, mapDetail.VirtualIP)
	}
}

func TestIPAllocation_PeerThenMap_NoOverlap(t *testing.T) {
	st := testIPAllocStore(t)
	g, err := st.CreateGroup("users", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	peerIP, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if err := st.CreatePeer(&Peer{
		Name: "alpha", DNSName: "alpha", WGIP: peerIP,
		PublicKey: "pk-alpha", PrivateKey: "sk-alpha", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	mapDetail, err := st.CreateServiceMap(MapInput{
		Slug:          "svc-b",
		TargetHost:    "127.0.0.1",
		AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}
	if mapDetail.VirtualIP == peerIP {
		t.Fatalf("map VIP %q collides with peer IP %q", mapDetail.VirtualIP, peerIP)
	}
}

func TestIPAllocation_Interleaved_NoOverlap(t *testing.T) {
	st := testIPAllocStore(t)
	g, err := st.CreateGroup("users", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	seen := map[string]string{
		"100.127.0.1": "hub",
	}

	add := func(ip, owner string) {
		t.Helper()
		if prev, ok := seen[ip]; ok {
			t.Fatalf("IP %q reused by %q (already used by %q)", ip, owner, prev)
		}
		seen[ip] = owner
	}

	peer1, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	add(peer1, "peer1")
	if err := st.CreatePeer(&Peer{
		Name: "p1", DNSName: "p1", WGIP: peer1,
		PublicKey: "pk1", PrivateKey: "sk1", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	map1, err := st.CreateServiceMap(MapInput{
		Slug: "m1", TargetHost: "127.0.0.1", AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}
	add(map1.VirtualIP, "map1")

	peer2, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	add(peer2, "peer2")
	if err := st.CreatePeer(&Peer{
		Name: "p2", DNSName: "p2", WGIP: peer2,
		PublicKey: "pk2", PrivateKey: "sk2", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	map2, err := st.CreateServiceMap(MapInput{
		Slug: "m2", TargetHost: "127.0.0.1", AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}
	add(map2.VirtualIP, "map2")
}

func TestIPAllocation_ReservesManualDNSRecord(t *testing.T) {
	st := testIPAllocStore(t)
	if err := st.CreateDNSRecord(&DNSRecord{
		Hostname: "legacy.internal",
		IP:       "100.127.0.2",
		Manual:   true,
	}); err != nil {
		t.Fatal(err)
	}

	ip, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip == "100.127.0.2" {
		t.Fatalf("allocated peer IP %q overlaps manual DNS record", ip)
	}

	vip, err := st.AllocateMapIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if vip == "100.127.0.2" {
		t.Fatalf("allocated map VIP %q overlaps manual DNS record", vip)
	}
}
