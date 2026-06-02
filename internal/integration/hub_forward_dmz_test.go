package integration

import (
	"fmt"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
)

func TestPortForwardDMZTCPSamePort(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "dmzhost", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	host := env.peerNamed("dmzhost")
	if host == nil {
		t.Fatal("missing peer dmzhost")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, host.Net, host.Peer.WGIP, backendPort, "dmz-ok")
	defer stopBackend()

	if _, err := env.store.UpsertPortForwardDMZ(repo.PortForwardDMZInput{
		TargetHost: domain.PeerFQDN("dmzhost"),
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, backendPort)
	body, err := peerHTTPGet(host.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub dmz: %v", err)
	}
	if body != "dmz-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestPortForwardDMZYieldToExplicitForward(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "dmzhost", GroupName: "default"},
		{Name: "other", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	dmzHost := env.peerNamed("dmzhost")
	other := env.peerNamed("other")
	if dmzHost == nil || other == nil {
		t.Fatal("missing peers")
	}

	dmzPort := freeTCPPort(t)
	stopDMZ := startPeerHTTPServer(t, dmzHost.Net, dmzHost.Peer.WGIP, dmzPort, "dmz-backend")
	defer stopDMZ()

	otherPort := freeTCPPort(t)
	stopOther := startPeerHTTPServer(t, other.Net, other.Peer.WGIP, otherPort, "forward-backend")
	defer stopOther()

	if _, err := env.store.UpsertPortForwardDMZ(repo.PortForwardDMZInput{
		TargetHost: domain.PeerFQDN("dmzhost"),
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	hubWebPort := config.DefaultPort
	if _, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: dmzPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: domain.PeerFQDN("other"),
		TargetPort: otherPort,
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, dmzPort)
	body, err := peerHTTPGet(dmzHost.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward override dmz: %v", err)
	}
	if body != "forward-backend" {
		t.Fatalf("body = %q, want forward-backend (explicit forward wins)", body)
	}
}
