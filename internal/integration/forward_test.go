package integration

import (
	"fmt"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/forward"
	"github.com/touken928/wirehub/internal/domain/peer"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/ingress"
)

func TestPortForwardTCPToPeerHostname(t *testing.T) {
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	app := env.peerByName("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, app.Net, app.Peer.WGIP, backendPort, "forwarded-peer")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: peer.PeerFQDN("app"),
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

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
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "svc", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	svc := env.peerByName("svc")
	if svc == nil {
		t.Fatal("missing peer svc")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, svc.Net, svc.Peer.WGIP, backendPort, "by-ip")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: svc.Peer.WGIP,
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

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
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "web", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	web := env.peerByName("web")
	if web == nil {
		t.Fatal("missing peer web")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, web.Net, web.Peer.WGIP, backendPort, "fqdn-ok")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	targetHost := fmt.Sprintf("web.%s", config.DNSDomain)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: targetHost,
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGet(web.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (fqdn target): %v", err)
	}
	if body != "fqdn-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestPortForwardUDPToPeer(t *testing.T) {
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	app := env.peerByName("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerUDPEcho(t, app.Net, app.Peer.WGIP, backendPort)
	defer stopBackend()

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoUDP,
		TargetHost: peer.PeerFQDN("app"),
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

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
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "app", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	app := env.peerByName("app")
	if app == nil {
		t.Fatal("missing peer app")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, app.Net, app.Peer.WGIP, backendPort, "enabled")
	defer stopBackend()

	listenPort1 := freeTCPPort(t)
	listenPort2 := freeTCPPort(t)
	rule, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort1,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: peer.PeerFQDN("app"),
		TargetPort: backendPort,
	})
	if err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url1 := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort1)
	body, err := peerHTTPGet(app.Net, url1, 5*time.Second)
	if err != nil {
		t.Fatalf("initial forward: %v", err)
	}
	if body != "enabled" {
		t.Fatalf("body = %q", body)
	}

	if _, err := env.store.UpdatePortForward(rule.ID, ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort2,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: peer.PeerFQDN("app"),
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	if _, err := peerHTTPGet(app.Net, url1, 2*time.Second); err == nil {
		t.Fatal("old listen port should stop after reapply")
	}
	url2 := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort2)
	body, err = peerHTTPGet(app.Net, url2, 5*time.Second)
	if err != nil {
		t.Fatalf("after listen port update: %v", err)
	}
	if body != "enabled" {
		t.Fatalf("body = %q", body)
	}
}
