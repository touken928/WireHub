package integration

import (
	"fmt"
	"net"
	"net/http"
	"testing"
	"time"

	mapdom "github.com/touken928/wirehub/internal/domain/map"
	"github.com/touken928/wirehub/internal/domain/forward"
	"github.com/touken928/wirehub/internal/domain/peer"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/ingress"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

func groupIDByName(t *testing.T, env *meshEnv, name string) uint {
	t.Helper()
	groups, err := env.store.ListGroups()
	if err != nil {
		t.Fatal(err)
	}
	for _, g := range groups {
		if g.Name == name {
			return g.ID
		}
	}
	t.Fatalf("group %q not found", name)
	return 0
}

func createMapToPeer(t *testing.T, env *meshEnv, slug, backendPeer, groupName string) *repo.MapDetail {
	t.Helper()
	detail, err := env.store.CreateServiceMap(repo.MapInput{
		Slug:          slug,
		TargetHost:    peer.PeerFQDN(backendPeer),
		AllowedGroups: []uint{groupIDByName(t, env, groupName)},
	})
	if err != nil {
		t.Fatal(err)
	}
	return detail
}

// startPeerHTTPService tries port 80 first (maps are same-port / default HTTP), then 8080.
func startPeerHTTPService(t *testing.T, tnet *netstack.Net, ip, response string) (port int, stop func()) {
	t.Helper()
	for _, port := range []int{80, 8080} {
		mux := http.NewServeMux()
		mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
			fmt.Fprint(w, response)
		})
		ln, err := tnet.ListenTCP(&net.TCPAddr{IP: net.ParseIP(ip), Port: port})
		if err != nil {
			continue
		}
		go http.Serve(ln, mux)
		return port, func() { _ = ln.Close() }
	}
	t.Skip("cannot bind peer HTTP on port 80 or 8080")
	return 0, nil
}

func assertMapHTTP(t *testing.T, client *netstack.Net, url, wantBody string) {
	t.Helper()
	body, err := peerHTTPGet(client, url, 5*time.Second)
	if err != nil {
		t.Fatalf("GET %s: %v", url, err)
	}
	if body != wantBody {
		t.Fatalf("GET %s body = %q, want %q", url, body, wantBody)
	}
}

func setupMapPeers(t *testing.T) (*meshEnv, *connectedPeer, *connectedPeer, func()) {
	t.Helper()
	env, _, cleanup := setupMesh(t, []peerSpec{
		{Name: "client", GroupName: "clients"},
		{Name: "backend", GroupName: "clients"},
	}, nil)
	env.connectAll(t)
	client := env.peerByName("client")
	backend := env.peerByName("backend")
	if client == nil || backend == nil {
		cleanup()
		t.Fatal("missing map test peers")
	}
	return env, client, backend, cleanup
}

func TestMapTCPToPeer(t *testing.T) {
	env, client, backend, cleanup := setupMapPeers(t)
	defer cleanup()

	svcMap := createMapToPeer(t, env, "lan", "backend", "clients")
	env.applyMaps(t)

	backendPort := freeTCPPort(t)
	stopBackend := startPeerHTTPServer(t, backend.Net, backend.Peer.WGIP, backendPort, "map-tcp")
	defer stopBackend()
	time.Sleep(200 * time.Millisecond)

	addr := fmt.Sprintf("%s:%d", svcMap.VirtualIP, backendPort)
	assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s/", addr), "map-tcp")

	vip, err := queryA(client.Net, env.hubIP, mapdom.MapFQDN(svcMap.Slug))
	if err != nil {
		t.Fatalf("dns: %v", err)
	}
	if vip != svcMap.VirtualIP {
		t.Fatalf("dns vip = %q want %q", vip, svcMap.VirtualIP)
	}
	assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", vip, backendPort), "map-tcp")
}

// TestMapTCPDefaultServicePort verifies same-port relay when the client uses the service's
// listening port (HTTP default :80 when bindable). Maps do not remap ports — VIP:N → target:N.
func TestMapTCPDefaultServicePort(t *testing.T) {
	env, client, backend, cleanup := setupMapPeers(t)
	defer cleanup()

	svcMap := createMapToPeer(t, env, "lan", "backend", "clients")
	env.applyMaps(t)

	servicePort, stopBackend := startPeerHTTPService(t, backend.Net, backend.Peer.WGIP, "map-default-port")
	defer stopBackend()
	time.Sleep(200 * time.Millisecond)

	t.Run("vip explicit port", func(t *testing.T) {
		assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", svcMap.VirtualIP, servicePort), "map-default-port")
	})

	t.Run("slug fqdn explicit port", func(t *testing.T) {
		assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", mapdom.MapFQDN(svcMap.Slug), servicePort), "map-default-port")
	})

	if servicePort == 80 {
		t.Run("slug fqdn implicit port 80", func(t *testing.T) {
			assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s/", mapdom.MapFQDN(svcMap.Slug)), "map-default-port")
		})
	}
}

// TestMapTCPSamePortRequired documents that the dial port must match the backend listen port.
func TestMapTCPSamePortRequired(t *testing.T) {
	env, client, backend, cleanup := setupMapPeers(t)
	defer cleanup()

	svcMap := createMapToPeer(t, env, "lan", "backend", "clients")
	env.applyMaps(t)

	const listenPort = 9090
	stopBackend := startPeerHTTPServer(t, backend.Net, backend.Peer.WGIP, listenPort, "map-9090")
	defer stopBackend()
	time.Sleep(200 * time.Millisecond)

	assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", svcMap.VirtualIP, listenPort), "map-9090")

	// Wrong port must not reach the backend (same-port relay, not port translation).
	if _, err := peerHTTPGet(client.Net, fmt.Sprintf("http://%s:80/", svcMap.VirtualIP), 2*time.Second); err == nil {
		t.Fatal("expected map dial to wrong port 80 to fail when backend listens on 9090")
	}
}

// TestMapTCPDirectBackendBaseline ensures the client can reach the backend before map assertions.
func TestMapTCPDirectBackendBaseline(t *testing.T) {
	_, client, backend, cleanup := setupMapPeers(t)
	defer cleanup()

	port, stop := startPeerHTTPService(t, backend.Net, backend.Peer.WGIP, "direct-ok")
	defer stop()
	time.Sleep(100 * time.Millisecond)

	assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", backend.Peer.WGIP, port), "direct-ok")
}

// TestMapTCPRuntimeSyncRegistersVIP reproduces production map CRUD: hub already running,
// map added via runtime sync (no CreateNetTUN restart). VIPs must be registered on the hub stack.
func TestMapTCPRuntimeSyncRegistersVIP(t *testing.T) {
	env, client, backend, cleanup := setupMapPeers(t)
	defer cleanup()

	svcMap := createMapToPeer(t, env, "lan", "backend", "clients")
	env.applyMapsRuntimeSync(t, true)

	servicePort, stopBackend := startPeerHTTPService(t, backend.Net, backend.Peer.WGIP, "map-runtime")
	defer stopBackend()
	time.Sleep(200 * time.Millisecond)

	assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", svcMap.VirtualIP, servicePort), "map-runtime")
}

// TestMapTCPImplicitPort80MissesNonStandardBackend documents a common misconfiguration:
// Maps preserve ports (VIP:N → target:N). http://slug.wirehub/ dials port 80; if the backend
// listens elsewhere, the map appears "broken" unless the client uses the matching port.
func TestMapTCPImplicitPort80MissesNonStandardBackend(t *testing.T) {
	env, client, backend, cleanup := setupMapPeers(t)
	defer cleanup()

	svcMap := createMapToPeer(t, env, "lan", "backend", "clients")
	env.applyMapsRuntimeSync(t, true)

	const backendPort = 8080
	stopBackend := startPeerHTTPServer(t, backend.Net, backend.Peer.WGIP, backendPort, "map-8080")
	defer stopBackend()
	time.Sleep(200 * time.Millisecond)

	assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", svcMap.VirtualIP, backendPort), "map-8080")

	if _, err := peerHTTPGet(client.Net, fmt.Sprintf("http://%s/", mapdom.MapFQDN(svcMap.Slug)), 2*time.Second); err == nil {
		t.Fatal("implicit port 80 should not reach backend listening on 8080 (same-port map, not port remap)")
	}
}

// TestMapTCPHostNetwork3389LikeForward covers LAN/host targets (e.g. 192.168.9.112:3389) where
// Forward listens on hub.wirehub:3389 but Map must dial the same client port to the target host.
func TestMapTCPHostNetwork3389LikeForward(t *testing.T) {
	env, client, _, cleanup := setupMapPeers(t)
	defer cleanup()

	const rdpPort = 3389
	stopHost, ok := tryStartHostHTTPServer(rdpPort, "rdp-via-map")
	if !ok {
		t.Skip("cannot bind host port 3389 for RDP map test")
	}
	defer stopHost()

	svcMap, err := env.store.CreateServiceMap(repo.MapInput{
		Slug:          "4080s",
		TargetHost:    "127.0.0.1",
		AllowedGroups: []uint{groupIDByName(t, env, "clients")},
	})
	if err != nil {
		t.Fatal(err)
	}
	env.applyMapsRuntimeSync(t, true)

	t.Run("map vip same port", func(t *testing.T) {
		assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", svcMap.VirtualIP, rdpPort), "rdp-via-map")
	})

	t.Run("map slug fqdn same port", func(t *testing.T) {
		assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", mapdom.MapFQDN("4080s"), rdpPort), "rdp-via-map")
	})

	t.Run("forward hub listen parity", func(t *testing.T) {
		if _, err := env.store.CreatePortForward(ingress.HubTunnelWebPort, repo.PortForwardInput{
			ListenPort: rdpPort,
			Protocol:   forward.ForwardProtoTCP,
			TargetHost: "127.0.0.1",
			TargetPort: rdpPort,
		}); err != nil {
			t.Fatal(err)
		}
		env.applyPortForwards(t)
		assertMapHTTP(t, client.Net, fmt.Sprintf("http://%s:%d/", env.hubIP, rdpPort), "rdp-via-map")
	})
}

func tryStartHostHTTPServer(port int, response string) (stop func(), ok bool) {
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, response)
	})
	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
	if err != nil {
		return nil, false
	}
	go http.Serve(ln, mux)
	return func() { _ = ln.Close() }, true
}
