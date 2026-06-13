package repo

import (
	"errors"
	"path/filepath"
	"strings"
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

func testIPAllocStoreWithSubnet(t *testing.T, subnet, hubIP, dnsIP string) *Store {
	t.Helper()
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	settings := &Settings{
		WGSubnet: subnet,
		HubIP:    hubIP,
		DNSIP:    dnsIP,
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

// --- New subnet-boundary tests ---

func TestIPAllocation_Subnet16_CrossesOctetBoundary(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "10.0.0.0/16", "10.0.0.1", "10.0.0.1")
	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Allocate and persist 300 IPs; without carry fix we'd run out after 254
	// (10.0.0.2–10.0.0.255) and get an error at the 255th
	ips := make(map[string]bool)
	for i := 0; i < 300; i++ {
		ip, err := st.AllocateIP("10.0.0.0/16", "10.0.0.1", "10.0.0.1")
		if err != nil {
			t.Fatalf("failed to allocate IP %d: %v", i, err)
		}
		if ips[ip] {
			t.Fatalf("duplicate IP %q at iteration %d", ip, i)
		}
		ips[ip] = true
		// Persist so the allocator tracks it
		if err := st.CreatePeer(&Peer{
			Name:       peerName(i),
			DNSName:    peerName(i),
			WGIP:       ip,
			PublicKey:  "pk-" + peerName(i),
			PrivateKey: "sk-" + peerName(i),
			GroupID:    g.ID,
			Enabled:    true,
		}); err != nil {
			t.Fatal(err)
		}
	}

	// Verify we have IPs in 10.0.1.x (must cross /24 boundary)
	hasSecondRange := false
	for ip := range ips {
		if len(ip) > 7 && ip[:7] == "10.0.1." {
			hasSecondRange = true
			break
		}
	}
	if !hasSecondRange {
		t.Fatal("expected at least one IP in 10.0.1.x range for /16 subnet")
	}
}

func TestIPAllocation_Subnet23_CrossesOctetBoundary(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "172.16.2.0/23", "172.16.2.1", "172.16.2.1")
	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// /23 has 512 IPs; allocate and persist 256 IPs — should cross into .3.x
	seen := map[string]bool{"172.16.2.1": true}
	for i := 0; i < 256; i++ {
		ip, err := st.AllocateIP("172.16.2.0/23", "172.16.2.1", "172.16.2.1")
		if err != nil {
			t.Fatalf("failed at iteration %d: %v", i, err)
		}
		if seen[ip] {
			t.Fatalf("duplicate IP %q at iteration %d", ip, i)
		}
		seen[ip] = true
		if err := st.CreatePeer(&Peer{
			Name:       peerName(i),
			DNSName:    peerName(i),
			WGIP:       ip,
			PublicKey:  "pk-" + peerName(i),
			PrivateKey: "sk-" + peerName(i),
			GroupID:    g.ID,
			Enabled:    true,
		}); err != nil {
			t.Fatal(err)
		}
	}

	// Verify at least one IP in 172.16.3.x
	hasDot3 := false
	for ip := range seen {
		if strings.HasPrefix(ip, "172.16.3.") {
			hasDot3 = true
			break
		}
	}
	if !hasDot3 {
		t.Fatal("expected at least one IP in 172.16.3.x range for /23 subnet")
	}
}

func TestIPAllocation_Subnet30_YieldsCorrectly(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// /30: .0 network, .1 hub, .2-.3 are hosts
	ip, err := st.AllocateIP("10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip != "10.0.0.2" {
		t.Fatalf("expected 10.0.0.2, got %q", ip)
	}
	if err := st.CreatePeer(&Peer{
		Name: "p1", DNSName: "p1", WGIP: ip,
		PublicKey: "pk1", PrivateKey: "sk1", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	// Second allocation should give .3
	ip2, err := st.AllocateIP("10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip2 != "10.0.0.3" {
		t.Fatalf("expected 10.0.0.3, got %q", ip2)
	}
}

func TestIPAllocation_NonDot0Subnet24(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "192.168.5.0/24", "192.168.5.1", "192.168.5.1")

	ip, err := st.AllocateIP("192.168.5.0/24", "192.168.5.1", "192.168.5.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip != "192.168.5.2" {
		t.Fatalf("expected 192.168.5.2, got %q", ip)
	}
}

func TestIPAllocation_NonDot0Subnet24_NonZeroBase(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "10.0.1.0/24", "10.0.1.1", "10.0.1.1")

	ip, err := st.AllocateIP("10.0.1.0/24", "10.0.1.1", "10.0.1.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip != "10.0.1.2" {
		t.Fatalf("expected 10.0.1.2, got %q", ip)
	}
}

func TestIPAllocation_ExhaustionReturnsError(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Allocate and persist .2
	ip, err := st.AllocateIP("10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if err := st.CreatePeer(&Peer{
		Name: "p1", DNSName: "p1", WGIP: ip,
		PublicKey: "pk1", PrivateKey: "sk1", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	// Allocate and persist .3
	ip2, err := st.AllocateIP("10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if err := st.CreatePeer(&Peer{
		Name: "p2", DNSName: "p2", WGIP: ip2,
		PublicKey: "pk2", PrivateKey: "sk2", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	// Third allocation should fail — /30 exhausted
	_, err = st.AllocateIP("10.0.0.0/30", "10.0.0.1", "10.0.0.1")
	if err == nil {
		t.Fatal("expected error for exhausted /30 subnet")
	}
}

func TestIPAllocation_PeerCollisionAvoided(t *testing.T) {
	st := testIPAllocStore(t)
	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Manually create a peer at 100.127.0.5
	peer := &Peer{
		Name: "collidee", DNSName: "collidee", WGIP: "100.127.0.5",
		PublicKey: "pk-col", PrivateKey: "sk-col", GroupID: g.ID, Enabled: true,
	}
	if err := st.CreatePeer(peer); err != nil {
		t.Fatal(err)
	}

	// Allocate should skip .5
	ip, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip == "100.127.0.5" {
		t.Fatal("allocator returned IP of existing peer")
	}
}

func TestIPAllocation_MapVIPCollisionAvoided(t *testing.T) {
	st := testIPAllocStore(t)
	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Create first map — persisted VIP prevents collision
	detail, err := st.CreateServiceMap(MapInput{
		Slug: "existing", TargetHost: "10.0.0.1", AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Allocate a peer IP — allocator reads all used IPs from DB (peers + maps)
	ip, err := st.AllocateIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if ip == detail.VirtualIP {
		t.Fatal("peer IP collided with map VIP")
	}

	// Persist the peer so its IP is tracked
	if err := st.CreatePeer(&Peer{
		Name: "p", DNSName: "p", WGIP: ip,
		PublicKey: "pk-p", PrivateKey: "sk-p", GroupID: g.ID, Enabled: true,
	}); err != nil {
		t.Fatal(err)
	}

	// Second map — allocator must avoid both the first VIP and the peer IP
	detail2, err := st.CreateServiceMap(MapInput{
		Slug: "new", TargetHost: "10.0.0.2", AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}
	if detail2.VirtualIP == detail.VirtualIP {
		t.Fatal("map VIP collided with another map VIP")
	}
	if detail2.VirtualIP == ip {
		t.Fatal("map VIP collided with peer IP")
	}
}

// peerName returns a unique peer name for a 0-based index.
func peerName(i int) string {
	names := []string{"a", "b", "c", "d", "e", "f", "g", "h", "i", "j"}
	return names[i%len(names)] + string(rune('0'+i/len(names)))
}

func TestIPAllocation_AllocateMapIP_ViaStore(t *testing.T) {
	st := testIPAllocStore(t)

	vip, err := st.AllocateMapIP("100.127.0.0/24", "100.127.0.1", "100.127.0.1")
	if err != nil {
		t.Fatal(err)
	}
	if vip == "" || vip == "100.127.0.1" {
		t.Fatalf("unexpected VIP %q", vip)
	}
}

func TestIPAllocation_WrapsError(t *testing.T) {
	st := testIPAllocStoreWithSubnet(t, "10.0.0.0/31", "10.0.0.1", "10.0.0.1")

	_, err := st.AllocateIP("10.0.0.0/31", "10.0.0.1", "10.0.0.1")
	if err == nil {
		t.Fatal("expected error for /31 subnet (no usable hosts)")
	}

	_, err = st.AllocateMapIP("10.0.0.0/31", "10.0.0.1", "10.0.0.1")
	if err == nil {
		t.Fatal("expected ErrMapIPUnavailable for /31 subnet")
	}
	if !errors.Is(err, ErrMapIPUnavailable) {
		t.Fatalf("expected ErrMapIPUnavailable, got %v", err)
	}
}
