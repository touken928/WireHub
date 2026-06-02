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

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/network"
)

func TestHubWebOnNetstack(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "wirehub-ok")
	}))
	defer backend.Close()

	_, portStr, _ := net.SplitHostPort(backend.Listener.Addr().String())
	webPort := mustAtoi(portStr)
	if _, err := network.StartHubWebServer(env.wgMgr.Net(), env.hubIP, webPort, backend.Config.Handler); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)

	for _, host := range []string{config.DNSDomain, env.hubIP} {
		t.Run(host, func(t *testing.T) {
			url := fmt.Sprintf("http://%s:%d", host, webPort)
			req, _ := http.NewRequestWithContext(context.Background(), http.MethodGet, url, nil)
			resp, err := httpViaNetstack(tnet, env.dnsIP, env.hubIP, req)
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

func TestHubWebViaWireGuardPeer(t *testing.T) {
	env, _, cleanup := setupHub(t)
	defer cleanup()

	const webPort = 18080
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "tunnel-ok")
	})
	if _, err := network.StartHubWebServer(env.wgMgr.Net(), env.hubIP, webPort, mux); err != nil {
		t.Fatal(err)
	}

	clientDev, clientNet, err := startWireGuardClientLegacy(t, env)
	if err != nil {
		t.Fatal(err)
	}
	defer clientDev.Close()

	if err := waitForHandshake(t, env.wgMgr, env.peerPubKey, 5*time.Second); err != nil {
		t.Fatal(err)
	}

	for _, host := range []string{config.DNSDomain, env.hubIP} {
		t.Run(host, func(t *testing.T) {
			url := fmt.Sprintf("http://%s:%d", host, webPort)
			req, _ := http.NewRequestWithContext(context.Background(), http.MethodGet, url, nil)
			resp, err := httpViaNetstack(clientNet, env.dnsIP, env.hubIP, req)
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
