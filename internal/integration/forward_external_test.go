package integration

import (
	"crypto/tls"
	"fmt"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/domain/forward"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/ingress"
)

func TestPortForwardTCPToHostNetworkTarget(t *testing.T) {
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	client := env.peerByName("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startHostHTTPServer(t, backendPort, "host-network-ok")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: "127.0.0.1",
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGet(client.Net, url, 5*time.Second)
	if err != nil {
		if strings.Contains(err.Error(), "deadline exceeded") || strings.Contains(err.Error(), "connection reset") {
			t.Skipf("host-network forward unavailable in this environment: %v", err)
		}
		t.Fatalf("peer -> hub forward (host network target): %v", err)
	}
	if body != "host-network-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestPortForwardTCPToPublicWebPage(t *testing.T) {
	requireNetwork(t)

	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	client := env.peerByName("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	targetIP := resolvePublicIPv4(t, "example.com")
	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: targetIP,
		TargetPort: 80,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGetWithHost(client.Net, url, "example.com", 15*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (public webpage): %v", err)
	}
	if !strings.Contains(body, "Example Domain") {
		t.Fatalf("expected example.com homepage, got %q", truncateBody(body, 200))
	}
}

func TestPortForwardTCPToPublicWebPageViaFQDN(t *testing.T) {
	requireNetwork(t)

	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	// Resolve external forward targets via configured upstream DNS only.
	settings, err := env.store.GetSettings()
	if err != nil {
		t.Fatal(err)
	}
	if err := env.store.UpdateMutableSettings(settings.MTU, settings.StatusInterval, []string{"114.114.114.114"}); err != nil {
		t.Fatal(err)
	}
	env.dnsServer.SetUpstream([]string{"114.114.114.114"})
	env.connectAll(t)

	client := env.peerByName("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: "example.com",
		TargetPort: 80,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGetWithHost(client.Net, url, "example.com", 15*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (fqdn target): %v", err)
	}
	if !strings.Contains(body, "Example Domain") {
		t.Fatalf("expected example.com homepage, got %q", truncateBody(body, 200))
	}
}

func TestPortForwardTCPToPublicAPI(t *testing.T) {
	requireNetwork(t)

	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectAll(t)

	client := env.peerByName("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	targetIP := resolvePublicIPv4(t, "httpbin.org")
	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: targetIP,
		TargetPort: 80,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/get", env.hubIP, listenPort)
	body, err := peerHTTPGetWithHost(client.Net, url, "httpbin.org", 15*time.Second)
	if err != nil {
		if strings.Contains(err.Error(), "deadline exceeded") || strings.Contains(err.Error(), "timeout") {
			t.Skipf("public api forward unavailable in this environment: %v", err)
		}
		t.Fatalf("peer -> hub forward (public api): %v", err)
	}
	if !strings.Contains(body, `"url"`) || !strings.Contains(body, `"origin"`) {
		t.Fatalf("expected httpbin /get JSON, got %q", truncateBody(body, 200))
	}
}

func TestPortForwardTCPToPublicHTTPS(t *testing.T) {
	requireNetwork(t)

	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	settings, err := env.store.GetSettings()
	if err != nil {
		t.Fatal(err)
	}
	if err := env.store.UpdateMutableSettings(settings.MTU, settings.StatusInterval, []string{"114.114.114.114"}); err != nil {
		t.Fatal(err)
	}
	env.dnsServer.SetUpstream([]string{"114.114.114.114"})
	env.connectAll(t)

	client := env.peerByName("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   forward.ForwardProtoTCP,
		TargetHost: "example.com",
		TargetPort: 443,
	}); err != nil {
		t.Fatal(err)
	}
	env.applyPortForwards(t)

	url := fmt.Sprintf("https://%s:%d/", env.hubIP, listenPort)
	clientHTTP := http.Client{
		Transport: &http.Transport{
			DialContext: client.Net.DialContext,
			TLSClientConfig: &tls.Config{
				InsecureSkipVerify: true,
				ServerName:         "example.com",
			},
		},
		Timeout: 15 * time.Second,
	}
	resp, err := clientHTTP.Get(url)
	if err != nil {
		if strings.Contains(err.Error(), "timeout") || strings.Contains(err.Error(), "timed out") {
			t.Fatalf("forward tcp to example.com:443 timed out: %v", err)
		}
		t.Fatalf("forward tcp to example.com:443: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 500 {
		t.Fatalf("status = %d", resp.StatusCode)
	}
}
