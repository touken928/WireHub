package integration

import (
	"fmt"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
)

func TestPortForwardTCPToPeerHostname(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	app := env.peerNamed("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, app.Net, app.Peer.WGIP, backendPort, "forwarded-peer")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	hubWebPort := config.DefaultPort
	if _, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: domain.PeerFQDN("app"),
		TargetPort: backendPort,
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGet(app.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward: %v", err)
	}
	if body != "forwarded-peer" {
		t.Fatalf("body = %q", body)
	}
}

func TestPortForwardTCPToPeerIP(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "svc", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	svc := env.peerNamed("svc")
	if svc == nil {
		t.Fatal("missing peer svc")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, svc.Net, svc.Peer.WGIP, backendPort, "by-ip")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	hubWebPort := config.DefaultPort
	if _, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: svc.Peer.WGIP,
		TargetPort: backendPort,
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGet(svc.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (target ip): %v", err)
	}
	if body != "by-ip" {
		t.Fatalf("body = %q", body)
	}
}

func TestPortForwardTCPViaFQDN(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "web", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	web := env.peerNamed("web")
	if web == nil {
		t.Fatal("missing peer web")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, web.Net, web.Peer.WGIP, backendPort, "fqdn-ok")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	targetHost := fmt.Sprintf("web.%s", config.DNSDomain)
	hubWebPort := config.DefaultPort
	if _, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: targetHost,
		TargetPort: backendPort,
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGet(web.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (fqdn target): %v", err)
	}
	if body != "fqdn-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestPortForwardTCPDisabled(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	app := env.peerNamed("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, app.Net, app.Peer.WGIP, backendPort, "disabled")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	hubWebPort := config.DefaultPort
	if _, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: domain.PeerFQDN("app"),
		TargetPort: backendPort,
		Enabled:    false,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	if _, err := peerHTTPGet(app.Net, url, 2*time.Second); err == nil {
		t.Fatal("expected disabled forward to refuse connections")
	}
}

func TestPortForwardUDPToPeer(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	app := env.peerNamed("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerUDPEcho(t, app.Net, app.Peer.WGIP, backendPort)
	defer stopBackend()

	listenPort := freeTCPPort(t)
	hubWebPort := config.DefaultPort
	if _, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoUDP,
		TargetHost: domain.PeerFQDN("app"),
		TargetPort: backendPort,
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	addr := fmt.Sprintf("%s:%d", env.hubIP, listenPort)
	got, err := udpRoundTrip(app.Net, addr, "ping-udp")
	if err != nil {
		t.Fatalf("udp forward: %v", err)
	}
	if got != "ping-udp" {
		t.Fatalf("echo = %q", got)
	}
}

func TestPortForwardReapplyAfterUpdate(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	app := env.peerNamed("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, app.Net, app.Peer.WGIP, backendPort, "enabled")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	hubWebPort := config.DefaultPort
	rule, err := env.store.CreatePortForward(hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: domain.PeerFQDN("app"),
		TargetPort: backendPort,
		Enabled:    false,
	})
	if err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	if _, err := peerHTTPGet(app.Net, url, 2*time.Second); err == nil {
		t.Fatal("expected forward to be off")
	}

	if _, err := env.store.UpdatePortForward(rule.ID, hubWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: domain.PeerFQDN("app"),
		TargetPort: backendPort,
		Enabled:    true,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	body, err := peerHTTPGet(app.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("after enable: %v", err)
	}
	if body != "enabled" {
		t.Fatalf("body = %q", body)
	}
}
