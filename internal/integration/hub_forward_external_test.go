package integration

import (
	"context"
	"crypto/tls"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/filter/l4"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

func requireNetwork(t *testing.T) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	conn, err := (&net.Dialer{}).DialContext(ctx, "tcp", "example.com:80")
	if err != nil {
		t.Skipf("network unavailable: %v", err)
	}
	_ = conn.Close()
}

func TestPortForwardTCPToHostNetworkTarget(t *testing.T) {
	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	client := env.peerNamed("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	backendPort := freeTCPPort(t)
	stopBackend := startHostHTTPServer(t, backendPort, "host-network-ok")
	defer stopBackend()

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(l4.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: "127.0.0.1",
		TargetPort: backendPort,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/", env.hubIP, listenPort)
	body, err := peerHTTPGet(client.Net, url, 5*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (host network target): %v", err)
	}
	if body != "host-network-ok" {
		t.Fatalf("body = %q", body)
	}
}

func startHostHTTPServer(t *testing.T, port int, response string) func() {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, response)
	})
	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
	if err != nil {
		t.Fatal(err)
	}
	go http.Serve(ln, mux)
	return func() { _ = ln.Close() }
}

func resolvePublicIPv4(t *testing.T, host string) string {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	ips, err := net.DefaultResolver.LookupIP(ctx, "ip4", host)
	if err != nil {
		t.Fatalf("resolve %s: %v", host, err)
	}
	if len(ips) == 0 {
		t.Fatalf("resolve %s: no A record", host)
	}
	return ips[0].String()
}

func peerHTTPGetWithHost(tnet *netstack.Net, url, host string, timeout time.Duration) (string, error) {
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return "", err
	}
	if host != "" {
		req.Host = host
	}
	client := http.Client{
		Transport: &http.Transport{DialContext: tnet.DialContext},
		Timeout:   timeout,
	}
	resp, err := client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	return string(body), nil
}

func TestPortForwardTCPToPublicWebPage(t *testing.T) {
	requireNetwork(t)

	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	client := env.peerNamed("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	targetIP := resolvePublicIPv4(t, "example.com")
	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(l4.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: targetIP,
		TargetPort: 80,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

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

	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
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
	env.connectPeers(t)

	client := env.peerNamed("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(l4.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: "example.com",
		TargetPort: 80,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

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

	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
		{Name: "client", GroupName: "default"},
	}, nil)
	defer cleanup()
	env.connectPeers(t)

	client := env.peerNamed("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	targetIP := resolvePublicIPv4(t, "httpbin.org")
	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(l4.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: targetIP,
		TargetPort: 80,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

	url := fmt.Sprintf("http://%s:%d/get", env.hubIP, listenPort)
	body, err := peerHTTPGetWithHost(client.Net, url, "httpbin.org", 15*time.Second)
	if err != nil {
		t.Fatalf("peer -> hub forward (public api): %v", err)
	}
	if !strings.Contains(body, `"url"`) || !strings.Contains(body, `"origin"`) {
		t.Fatalf("expected httpbin /get JSON, got %q", truncateBody(body, 200))
	}
}

func TestPortForwardTCPToPublicHTTPS(t *testing.T) {
	requireNetwork(t)

	env, _, cleanup := setupPeerMesh(t, []meshPeerSpec{
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
	env.connectPeers(t)

	client := env.peerNamed("client")
	if client == nil {
		t.Fatal("missing peer client")
	}

	listenPort := freeTCPPort(t)
	if _, err := env.store.CreatePortForward(l4.HubTunnelWebPort, repo.PortForwardInput{
		ListenPort: listenPort,
		Protocol:   domain.ForwardProtoTCP,
		TargetHost: "example.com",
		TargetPort: 443,
	}); err != nil {
		t.Fatal(err)
	}
	env.syncPortForwards(t)

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

func truncateBody(body string, max int) string {
	if len(body) <= max {
		return body
	}
	return body[:max] + "…"
}
