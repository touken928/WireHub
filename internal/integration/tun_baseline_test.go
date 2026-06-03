package integration

import (
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/vpn/filter"
	"github.com/touken928/wirehub/internal/vpn/filter/l4"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/wg"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

// TestWireGuardTCPBaseline mirrors wireguard-go netstack http_server/http_client examples.
func TestWireGuardTCPBaseline(t *testing.T) {
	hubPort := freeUDPPort(t)

	hubPriv, hubPub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	peerPriv, peerPub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	hubPrivHex, _ := wg.KeyToHex(hubPriv)
	hubPubHex, _ := wg.KeyToHex(hubPub)
	peerPrivHex, _ := wg.KeyToHex(peerPriv)
	peerPubHex, _ := wg.KeyToHex(peerPub)

	hubTun, hubNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		1420,
	)
	if err != nil {
		t.Fatal(err)
	}
	hubDev := device.NewDevice(hubTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	if err := hubDev.IpcSet(fmt.Sprintf("private_key=%s\nlisten_port=%d\n", hubPrivHex, hubPort)); err != nil {
		t.Fatal(err)
	}
	if err := hubDev.IpcSet(fmt.Sprintf("public_key=%s\nallowed_ip=10.8.0.2/32\n", peerPubHex)); err != nil {
		t.Fatal(err)
	}
	if err := hubDev.Up(); err != nil {
		t.Fatal(err)
	}
	defer hubDev.Close()

	ln, err := hubNet.ListenTCP(&net.TCPAddr{IP: net.ParseIP("10.8.0.1"), Port: 18081})
	if err != nil {
		t.Fatal(err)
	}
	go http.Serve(ln, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "baseline-ok")
	}))

	clientTun, clientNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr("10.8.0.2")},
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		1420,
	)
	if err != nil {
		t.Fatal(err)
	}
	clientDev := device.NewDevice(clientTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	if err := clientDev.IpcSet(fmt.Sprintf(`private_key=%s
public_key=%s
endpoint=127.0.0.1:%d
allowed_ip=10.8.0.0/24
persistent_keepalive_interval=1
`, peerPrivHex, hubPubHex, hubPort)); err != nil {
		t.Fatal(err)
	}
	if err := clientDev.Up(); err != nil {
		t.Fatal(err)
	}
	defer clientDev.Close()

	time.Sleep(300 * time.Millisecond)

	client := http.Client{
		Transport: &http.Transport{DialContext: clientNet.DialContext},
		Timeout:   5 * time.Second,
	}
	resp, err := client.Get("http://10.8.0.1:18081/")
	if err != nil {
		t.Fatalf("baseline tcp: %v", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if string(body) != "baseline-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestWireGuardTCPWithFilterAndForwarding(t *testing.T) {
	hubPort := freeUDPPort(t)

	hubPriv, hubPub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	peerPriv, peerPub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	peerPrivHex, _ := wg.KeyToHex(peerPriv)
	hubPubHex, _ := wg.KeyToHex(hubPub)

	hubMgr, err := wg.NewManager("10.8.0.1", "10.8.0.1", hubPort, 1420)
	if err != nil {
		t.Fatal(err)
	}
	if err := hubMgr.ConfigureServer(hubPriv, hubPort); err != nil {
		t.Fatal(err)
	}
	if err := hubMgr.Up(); err != nil {
		t.Fatal(err)
	}
	defer hubMgr.Down()

	if err := hubMgr.SyncPeer(&repo.Peer{PublicKey: peerPub, WGIP: "10.8.0.2", Enabled: true}); err != nil {
		t.Fatal(err)
	}

	if err := filter.EnableForwarding(hubMgr.Net()); err != nil {
		t.Fatal(err)
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "filtered-ok")
	})
	if _, err := l4.StartWebServer(hubMgr.Net(), "10.8.0.1", 18082, mux); err != nil {
		t.Fatal(err)
	}

	clientTun, clientNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr("10.8.0.2")},
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		1420,
	)
	if err != nil {
		t.Fatal(err)
	}
	clientDev := device.NewDevice(clientTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	if err := clientDev.IpcSet(fmt.Sprintf(`private_key=%s
public_key=%s
endpoint=127.0.0.1:%d
allowed_ip=10.8.0.0/24
persistent_keepalive_interval=1
`, peerPrivHex, hubPubHex, hubPort)); err != nil {
		t.Fatal(err)
	}
	if err := clientDev.Up(); err != nil {
		t.Fatal(err)
	}
	defer clientDev.Close()

	time.Sleep(300 * time.Millisecond)

	client := http.Client{
		Transport: &http.Transport{DialContext: clientNet.DialContext},
		Timeout:   5 * time.Second,
	}
	resp, err := client.Get("http://10.8.0.1:18082/")
	if err != nil {
		t.Fatalf("filtered tcp: %v", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if string(body) != "filtered-ok" {
		t.Fatalf("body = %q", body)
	}
}

func TestWireGuardTCPWithEnableForwardingOnly(t *testing.T) {
	hubPort := freeUDPPort(t)

	hubPriv, hubPub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	peerPriv, peerPub, err := wg.GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	hubPrivHex, _ := wg.KeyToHex(hubPriv)
	hubPubHex, _ := wg.KeyToHex(hubPub)
	peerPrivHex, _ := wg.KeyToHex(peerPriv)
	peerPubHex, _ := wg.KeyToHex(peerPub)

	hubTun, hubNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		1420,
	)
	if err != nil {
		t.Fatal(err)
	}
	if err := filter.EnableForwarding(hubNet); err != nil {
		t.Fatal(err)
	}
	hubDev := device.NewDevice(hubTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	if err := hubDev.IpcSet(fmt.Sprintf("private_key=%s\nlisten_port=%d\n", hubPrivHex, hubPort)); err != nil {
		t.Fatal(err)
	}
	if err := hubDev.IpcSet(fmt.Sprintf("public_key=%s\nallowed_ip=10.8.0.2/32\n", peerPubHex)); err != nil {
		t.Fatal(err)
	}
	if err := hubDev.Up(); err != nil {
		t.Fatal(err)
	}
	defer hubDev.Close()

	ln, err := hubNet.ListenTCP(&net.TCPAddr{IP: net.ParseIP("10.8.0.1"), Port: 18083})
	if err != nil {
		t.Fatal(err)
	}
	go http.Serve(ln, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, "fwd-ok")
	}))

	clientTun, clientNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr("10.8.0.2")},
		[]netip.Addr{netip.MustParseAddr("10.8.0.1")},
		1420,
	)
	if err != nil {
		t.Fatal(err)
	}
	clientDev := device.NewDevice(clientTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	if err := clientDev.IpcSet(fmt.Sprintf(`private_key=%s
public_key=%s
endpoint=127.0.0.1:%d
allowed_ip=10.8.0.0/24
persistent_keepalive_interval=1
`, peerPrivHex, hubPubHex, hubPort)); err != nil {
		t.Fatal(err)
	}
	if err := clientDev.Up(); err != nil {
		t.Fatal(err)
	}
	defer clientDev.Close()

	time.Sleep(300 * time.Millisecond)

	client := http.Client{
		Transport: &http.Transport{DialContext: clientNet.DialContext},
		Timeout:   5 * time.Second,
	}
	resp, err := client.Get("http://10.8.0.1:18083/")
	if err != nil {
		t.Fatalf("forwarding-only tcp: %v", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if string(body) != "fwd-ok" {
		t.Fatalf("body = %q", body)
	}
}
