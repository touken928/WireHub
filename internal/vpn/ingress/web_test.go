package ingress

import (
	"fmt"
	"io"
	"net/http"
	"net/netip"
	"testing"
	"time"
)

func TestStartWebServerSystemListen(t *testing.T) {
	hub := netip.MustParseAddr("100.127.0.1")
	const webPort = 19080

	tnet, cleanup := newTestNetstack(t, hub)
	defer cleanup()

	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "system-web-ok")
	})
	srv, err := StartWebServer(tnet, hub.String(), webPort, mux)
	if err != nil {
		t.Fatal(err)
	}
	defer srv.Close()
	time.Sleep(50 * time.Millisecond)

	url := fmt.Sprintf("http://%s/", netip.AddrPortFrom(hub, webPort))
	resp, err := testHTTPClient(tnet).Get(url)
	if err != nil {
		t.Fatalf("GET hub web port: %v", err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatal(err)
	}
	if string(body) != "system-web-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestStartWebServerRejectsInvalidHubIP(t *testing.T) {
	tnet, cleanup := newTestNetstack(t, netip.MustParseAddr("100.127.0.1"))
	defer cleanup()

	if _, err := StartWebServer(tnet, "not-an-ip", 8443, http.NewServeMux()); err == nil {
		t.Fatal("expected error for invalid hub IP")
	}
}
