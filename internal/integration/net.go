package integration

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"testing"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

func freeUDPPort(t *testing.T) int {
	t.Helper()
	ln, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4zero, Port: 0})
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	return ln.LocalAddr().(*net.UDPAddr).Port
}

func freeTCPPort(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	return ln.Addr().(*net.TCPAddr).Port
}

func atoiPort(s string) int {
	var n int
	fmt.Sscanf(s, "%d", &n)
	return n
}

func httpViaNetstack(tnet *netstack.Net, dnsIP, hubIP string, req *http.Request) (*http.Response, error) {
	host := req.URL.Hostname()
	port := req.URL.Port()
	if port == "" {
		port = "80"
	}
	ip := hubIP
	if host != hubIP {
		var err error
		ip, err = queryA(tnet, dnsIP, host)
		if err != nil {
			return nil, err
		}
	}
	addr := net.JoinHostPort(ip, port)
	transport := &http.Transport{
		DialContext: func(ctx context.Context, network, _ string) (net.Conn, error) {
			return tnet.DialContext(ctx, network, addr)
		},
	}
	client := http.Client{Transport: transport, Timeout: 5 * time.Second}
	return client.Do(req)
}

func peerHTTPGet(tnet *netstack.Net, url string, timeout time.Duration) (string, error) {
	client := http.Client{
		Transport: &http.Transport{DialContext: tnet.DialContext},
		Timeout:   timeout,
	}
	resp, err := client.Get(url)
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

func startHostUDPEcho(t *testing.T, port int) func() {
	t.Helper()
	pc, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: port})
	if err != nil {
		t.Fatal(err)
	}
	go func() {
		buf := make([]byte, 2048)
		for {
			n, addr, err := pc.ReadFrom(buf)
			if err != nil {
				return
			}
			if _, err := pc.WriteTo(buf[:n], addr); err != nil {
				return
			}
		}
	}()
	return func() { _ = pc.Close() }
}

func startPeerUDPEcho(t *testing.T, tnet *netstack.Net, ip string, port int) func() {
	t.Helper()
	pc, err := tnet.ListenUDP(&net.UDPAddr{IP: net.ParseIP(ip), Port: port})
	if err != nil {
		t.Fatal(err)
	}
	go func() {
		buf := make([]byte, 2048)
		for {
			n, addr, err := pc.ReadFrom(buf)
			if err != nil {
				return
			}
			if _, err := pc.WriteTo(buf[:n], addr); err != nil {
				return
			}
		}
	}()
	return func() { _ = pc.Close() }
}

func udpRoundTrip(tnet *netstack.Net, addr, payload string) (string, error) {
	raddr, err := net.ResolveUDPAddr("udp", addr)
	if err != nil {
		return "", err
	}
	conn, err := tnet.DialUDP(nil, raddr)
	if err != nil {
		return "", err
	}
	defer conn.Close()
	if _, err := conn.Write([]byte(payload)); err != nil {
		return "", err
	}
	_ = conn.SetReadDeadline(time.Now().Add(3 * time.Second))
	buf := make([]byte, 2048)
	n, err := conn.Read(buf)
	if err != nil {
		return "", err
	}
	return string(buf[:n]), nil
}

func startPeerHTTPServer(t *testing.T, tnet *netstack.Net, ip string, port int, response string) func() {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, response)
	})
	ln, err := tnet.ListenTCP(&net.TCPAddr{IP: net.ParseIP(ip), Port: port})
	if err != nil {
		t.Fatal(err)
	}
	go http.Serve(ln, mux)
	return func() { _ = ln.Close() }
}
