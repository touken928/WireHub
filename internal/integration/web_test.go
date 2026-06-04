package integration

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/domain/peer"
	"github.com/touken928/wirehub/internal/vpn/ingress"
)

func TestWebOnHubNetstack(t *testing.T) {
	env, hubNet, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()

	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "wirehub-ok")
	}))
	defer backend.Close()

	_, portStr, _ := net.SplitHostPort(backend.Listener.Addr().String())
	webPort := atoiPort(portStr)
	if _, err := ingress.StartWebServer(env.wgMgr.Net(), env.hubIP, webPort, backend.Config.Handler); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)

	for _, host := range []string{peer.HubFQDN(), env.hubIP} {
		t.Run(host, func(t *testing.T) {
			url := fmt.Sprintf("http://%s:%d", host, webPort)
			req, _ := http.NewRequestWithContext(context.Background(), http.MethodGet, url, nil)
			resp, err := httpViaNetstack(hubNet, env.dnsIP, env.hubIP, req)
			if err != nil {
				t.Fatalf("http via netstack (%s): %v", host, err)
			}
			defer resp.Body.Close()
			body, _ := io.ReadAll(resp.Body)
			if string(body) != "wirehub-ok" {
				t.Fatalf("body = %q", body)
			}
		})
	}
}

func TestWebViaWireGuardPeer(t *testing.T) {
	env, _, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()

	const webPort = 18080
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "tunnel-ok")
	})
	if _, err := ingress.StartWebServer(env.wgMgr.Net(), env.hubIP, webPort, mux); err != nil {
		t.Fatal(err)
	}

	env.connectAll(t)
	client := env.peerByName("touken")
	if client == nil || client.Net == nil {
		t.Fatal("peer client not connected")
	}

	for _, host := range []string{peer.HubFQDN(), env.hubIP} {
		t.Run(host, func(t *testing.T) {
			url := fmt.Sprintf("http://%s:%d", host, webPort)
			req, _ := http.NewRequestWithContext(context.Background(), http.MethodGet, url, nil)
			resp, err := httpViaNetstack(client.Net, env.dnsIP, env.hubIP, req)
			if err != nil {
				t.Fatalf("http via wg peer (%s): %v", host, err)
			}
			defer resp.Body.Close()
			body, _ := io.ReadAll(resp.Body)
			if string(body) != "tunnel-ok" {
				t.Fatalf("body = %q", body)
			}
		})
	}
}
