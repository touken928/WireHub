package integration

import (
	"fmt"
	"testing"

	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
)

func TestMapUDPToHostNetwork(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "client", GroupName: "clients"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	client := env.peerNamed("client")
	if client == nil {
		t.Fatal("missing peer client")
	}
	groups, err := env.store.ListGroups()
	if err != nil {
		t.Fatal(err)
	}
	var groupID uint
	for _, g := range groups {
		if g.Name == "clients" {
			groupID = g.ID
			break
		}
	}
	if groupID == 0 {
		t.Fatal("clients group not found")
	}

	backendPort := freeUDPPort(t)
	stopBackend := startHostUDPEcho(t, backendPort)
	defer stopBackend()

	svcMap, err := env.store.CreateServiceMap(repo.MapInput{
		Slug:          "lan",
		TargetHost:    "127.0.0.1",
		AllowedGroups: []uint{groupID},
	})
	if err != nil {
		t.Fatal(err)
	}
	env.syncMaps(t)

	addr := fmt.Sprintf("%s:%d", svcMap.VirtualIP, backendPort)
	got, err := udpRoundTrip(client.Net, addr, "map-udp")
	if err != nil {
		t.Fatalf("map udp: %v", err)
	}
	if got != "map-udp" {
		t.Fatalf("echo = %q", got)
	}

	vip, err := queryA(client.Net, env.hubIP, domain.MapFQDN(svcMap.Slug))
	if err != nil {
		t.Fatalf("dns: %v", err)
	}
	if vip != svcMap.VirtualIP {
		t.Fatalf("dns vip = %q want %q", vip, svcMap.VirtualIP)
	}
	addrFQDN := fmt.Sprintf("%s:%d", vip, backendPort)
	got, err = udpRoundTrip(client.Net, addrFQDN, "map-udp-dns")
	if err != nil {
		t.Fatalf("map udp via dns: %v", err)
	}
	if got != "map-udp-dns" {
		t.Fatalf("echo = %q", got)
	}
}
