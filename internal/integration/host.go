package integration

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"

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
		if strings.Contains(err.Error(), "timeout") || strings.Contains(err.Error(), "i/o timeout") {
			t.Skipf("resolve %s: %v", host, err)
		}
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

func truncateBody(body string, max int) string {
	if len(body) <= max {
		return body
	}
	return body[:max] + "…"
}
