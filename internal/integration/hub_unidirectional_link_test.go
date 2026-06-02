package integration

import (
	"fmt"
	"testing"
	"time"
)

func (env *peerMeshEnv) setUnidirectionalLink(t *testing.T, fromGroup, toGroup string) {
	t.Helper()
	groups, err := env.store.ListGroups()
	if err != nil {
		t.Fatal(err)
	}
	var fromID, toID uint
	for _, g := range groups {
		if g.Name == fromGroup {
			fromID = g.ID
		}
		if g.Name == toGroup {
			toID = g.ID
		}
	}
	if fromID == 0 || toID == 0 {
		t.Fatalf("groups %q -> %q not found", fromGroup, toGroup)
	}
	if err := env.store.UpsertGroupLink(fromID, toID, false); err != nil {
		t.Fatal(err)
	}
	if err := reloadAccessRules(env.store, env.wgMgr); err != nil {
		t.Fatal(err)
	}
}

func TestUnidirectionalLinkTransparentSNAT(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "client", GroupName: "active"},
		{Name: "svc", GroupName: "backend"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)
	env.setUnidirectionalLink(t, "active", "backend")

	svc := env.peerNamed("svc")
	client := env.peerNamed("client")
	if svc == nil || client == nil {
		t.Fatal("missing peers")
	}

	port := freeTCPPort(t)
	stop := startPeerHTTPServer(t, svc.Net, svc.Peer.WGIP, port, "backend-ok")
	defer stop()

	// Client dials target peer IP:port directly (hub SNAT is transparent).
	url := fmt.Sprintf("http://%s:%d/", svc.Peer.WGIP, port)
	body, err := peerHTTPGet(client.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("client -> svc via hub SNAT: %v", err)
	}
	if body != "backend-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestUnidirectionalLinkCannotInitiateReverse(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "client", GroupName: "active"},
		{Name: "svc", GroupName: "backend"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)
	env.setUnidirectionalLink(t, "active", "backend")

	svc := env.peerNamed("svc")
	client := env.peerNamed("client")
	if svc == nil || client == nil {
		t.Fatal("missing peers")
	}

	port := freeTCPPort(t)
	stop := startPeerHTTPServer(t, client.Net, client.Peer.WGIP, port, "client-ok")
	defer stop()

	url := fmt.Sprintf("http://%s:%d/", client.Peer.WGIP, port)
	if _, err := peerHTTPGet(svc.Net, url, 3*time.Second); err == nil {
		t.Fatal("backend group must not reach active group directly")
	}
}
