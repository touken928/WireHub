package integration

import (
	"fmt"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/store"
)

func TestPeerInterconnect(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, peerSpecs("alice", "bob"))
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

func TestPeerInterconnectExcluded(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []store.Peer{
		{Name: "alice", AccessExclude: []string{"bob"}},
		{Name: "bob"},
		{Name: "charlie"},
	})
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
			t.Fatal("expected alice -> bob to fail when bob is excluded")
		}
	})

	t.Run("bob blocked from alice", func(t *testing.T) {
		alicePort := freeTCPPort(t)
		stopAlice := startPeerHTTPServer(t, alice.Net, alice.Peer.WGIP, alicePort, "alice-ok")
		defer stopAlice()

		url := fmt.Sprintf("http://%s:%d/", alice.Peer.WGIP, alicePort)
		if _, err := peerHTTPGet(bob.Net, url, 2*time.Second); err == nil {
			t.Fatal("expected bob -> alice to fail when alice excludes bob")
		}
	})

	t.Run("alice can reach charlie", func(t *testing.T) {
		url := fmt.Sprintf("http://%s:%d/", charlie.Peer.WGIP, charliePort)
		body, err := peerHTTPGet(alice.Net, url, 5*time.Second)
		if err != nil {
			t.Fatalf("alice -> charlie: %v", err)
		}
		if body != "charlie-ok" {
			t.Fatalf("body = %q", body)
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

func peerSpecs(names ...string) []store.Peer {
	out := make([]store.Peer, len(names))
	for i, name := range names {
		out[i] = store.Peer{Name: name}
	}
	return out
}
