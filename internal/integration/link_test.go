package integration

import (
	"fmt"
	"testing"
	"time"
)

func TestUnidirectionalLinkTransparentSNAT(t *testing.T) {
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "active"},
		{Name: "svc", GroupName: "backend"},
	}, nil)
	defer cleanup()
	env.connectAll(t)
	env.setUnidirectionalLink(t, "active", "backend")

	svc := env.peerByName("svc")
	client := env.peerByName("client")
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
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "active"},
		{Name: "svc", GroupName: "backend"},
	}, nil)
	defer cleanup()
	env.connectAll(t)
	env.setUnidirectionalLink(t, "active", "backend")

	svc := env.peerByName("svc")
	client := env.peerByName("client")
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
