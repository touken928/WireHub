package integration

import (
	"fmt"
	"testing"
	"time"
)

func TestPeerInterconnect(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "alice", GroupName: "team"},
		{Name: "bob", GroupName: "team"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	alice := env.peerNamed("alice")
	bob := env.peerNamed("bob")
	if alice == nil || bob == nil {
		t.Fatal("missing test peers")
	}

	port := freeTCPPort(t)
	stop := startPeerHTTPServer(t, bob.Net, bob.Peer.WGIP, port, "peer-ok")
	defer stop()

	url := fmt.Sprintf("http://%s:%d/", bob.Peer.WGIP, port)
	body, err := peerHTTPGet(alice.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("alice -> bob: %v", err)
	}
	if body != "peer-ok" {
		t.Fatalf("alice -> bob body = %q", body)
	}
}

func TestPeerInterconnectGroupLinks(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "alice", GroupName: "alice"},
		{Name: "bob", GroupName: "bob"},
		{Name: "charlie", GroupName: "charlie"},
	}, [][2]string{{"bob", "charlie"}})
	defer cleanup()
	env.connectPeers(t)

	alice := env.peerNamed("alice")
	bob := env.peerNamed("bob")
	charlie := env.peerNamed("charlie")
	if alice == nil || bob == nil || charlie == nil {
		t.Fatal("missing test peers")
	}

	bobPort := freeTCPPort(t)
	stopBob := startPeerHTTPServer(t, bob.Net, bob.Peer.WGIP, bobPort, "bob-ok")
	defer stopBob()

	charliePort := freeTCPPort(t)
	stopCharlie := startPeerHTTPServer(t, charlie.Net, charlie.Peer.WGIP, charliePort, "charlie-ok")
	defer stopCharlie()

	t.Run("alice blocked from bob", func(t *testing.T) {
		url := fmt.Sprintf("http://%s:%d/", bob.Peer.WGIP, bobPort)
		if _, err := peerHTTPGet(alice.Net, url, 2*time.Second); err == nil {
			t.Fatal("expected alice -> bob to fail without group link")
		}
	})

	t.Run("bob blocked from alice", func(t *testing.T) {
		alicePort := freeTCPPort(t)
		stopAlice := startPeerHTTPServer(t, alice.Net, alice.Peer.WGIP, alicePort, "alice-ok")
		defer stopAlice()

		url := fmt.Sprintf("http://%s:%d/", alice.Peer.WGIP, alicePort)
		if _, err := peerHTTPGet(bob.Net, url, 2*time.Second); err == nil {
			t.Fatal("expected bob -> alice to fail without group link")
		}
	})

	t.Run("alice blocked from charlie", func(t *testing.T) {
		url := fmt.Sprintf("http://%s:%d/", charlie.Peer.WGIP, charliePort)
		if _, err := peerHTTPGet(alice.Net, url, 2*time.Second); err == nil {
			t.Fatal("expected alice -> charlie to fail without group link")
		}
	})

	t.Run("charlie can reach bob", func(t *testing.T) {
		url := fmt.Sprintf("http://%s:%d/", bob.Peer.WGIP, bobPort)
		body, err := peerHTTPGet(charlie.Net, url, 5*time.Second)
		if err != nil {
			t.Fatalf("charlie -> bob: %v", err)
		}
		if body != "bob-ok" {
			t.Fatalf("body = %q", body)
		}
	})
}
