package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

func validConfig() string {
	key := base64.StdEncoding.EncodeToString(make([]byte, 32))
	return fmt.Sprintf("[Interface]\nPrivateKey = %s\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = %s\nEndpoint = 127.0.0.1:51820\nAllowedIPs = 10.0.0.0/24\n", key, key)
}

func TestParseConfigRejectsMalformedAndInjection(t *testing.T) {
	valid := validConfig()
	for name, value := range map[string]string{
		"empty": "", "missing field": strings.Replace(valid, "PrivateKey = "+base64.StdEncoding.EncodeToString(make([]byte, 32))+"\n", "", 1),
		"missing section newline": strings.Replace(valid, "[Peer]\n", "[Peer] PublicKey = ", 1),
		"bad key length":          strings.Replace(valid, base64.StdEncoding.EncodeToString(make([]byte, 32)), base64.StdEncoding.EncodeToString(make([]byte, 31)), 1),
		"injected field":          strings.Replace(valid, "Endpoint = 127.0.0.1:51820", "Endpoint = 127.0.0.1:51820\nAllowedIPs = 0.0.0.0/0", 1),
		"unknown field":           strings.Replace(valid, "Address = 10.0.0.2/32", "Address = 10.0.0.2/32\nPreUp = touch /tmp/pwn", 1),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := parseConfig(value); err == nil {
				t.Fatal("expected invalid config")
			}
		})
	}
	if _, err := parseConfig(valid); err != nil {
		t.Fatalf("valid config rejected: %v", err)
	}
}

func TestHTTPAuthorizationAndPublicHealth(t *testing.T) {
	mux := httpMux(&client{}, "secret")
	for _, tc := range []struct {
		path, auth string
		want       int
	}{{"/health", "", 200}, {"/status", "", 401}, {"/not-found", "", 401}, {"/status", "Bearer wrong", 401}, {"/status", "Bearer secret", 200}} {
		r := httptest.NewRequest("GET", tc.path, nil)
		if tc.auth != "" {
			r.Header.Set("Authorization", tc.auth)
		}
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, r)
		if w.Code != tc.want {
			t.Errorf("%s auth %q: got %d want %d", tc.path, tc.auth, w.Code, tc.want)
		}
		if tc.path == "/status" && w.Code == 200 {
			var got map[string]json.RawMessage
			if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
				t.Fatal(err)
			}
			if len(got) != 2 {
				t.Fatalf("status contains unexpected fields: %s", w.Body.String())
			}
		}
	}
}

func TestUserNetstackTCPAndUDPEcho(t *testing.T) {
	_, stack, err := netstack.CreateNetTUN([]netip.Addr{netip.MustParseAddr("10.0.0.2")}, nil, 1420)
	if err != nil {
		t.Fatal(err)
	}
	c := &client{stack: stack, tun: netip.MustParseAddr("10.0.0.2")}
	tcp, err := stack.ListenTCPAddrPort(netip.MustParseAddrPort("10.0.0.2:19081"))
	if err != nil {
		t.Fatal(err)
	}
	udp, err := stack.ListenUDPAddrPort(netip.MustParseAddrPort("10.0.0.2:19081"))
	if err != nil {
		t.Fatal(err)
	}
	s := &server{tcp: tcp, udp: udp, done: make(chan struct{})}
	go c.tcpEcho(s, tcp)
	go c.udpEcho(s, udp)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	tcpClient, err := stack.DialContextTCPAddrPort(ctx, netip.MustParseAddrPort("10.0.0.2:19081"))
	if err != nil {
		t.Fatal(err)
	}
	defer tcpClient.Close()
	_ = tcpClient.SetDeadline(time.Now().Add(2 * time.Second))
	if _, err = tcpClient.Write([]byte("tcp echo")); err != nil {
		t.Fatal(err)
	}
	tb := make([]byte, 8)
	if _, err = io.ReadFull(tcpClient, tb); err != nil || string(tb) != "tcp echo" {
		t.Fatalf("tcp echo %q, %v", tb, err)
	}
	udpClient, err := stack.DialUDPAddrPort(netip.AddrPort{}, netip.MustParseAddrPort("10.0.0.2:19081"))
	if err != nil {
		t.Fatal(err)
	}
	defer udpClient.Close()
	_ = udpClient.SetDeadline(time.Now().Add(2 * time.Second))
	if _, err = udpClient.Write([]byte("udp echo")); err != nil {
		t.Fatal(err)
	}
	ub := make([]byte, 16)
	n, err := udpClient.Read(ub)
	if err != nil || string(ub[:n]) != "udp echo" {
		t.Fatalf("udp echo %q, %v", ub[:n], err)
	}
	s.close()
}

func TestPersistentTCPControlConnection(t *testing.T) {
	_, stack, err := netstack.CreateNetTUN([]netip.Addr{netip.MustParseAddr("10.0.0.2")}, nil, 1420)
	if err != nil {
		t.Fatal(err)
	}
	tcp, err := stack.ListenTCPAddrPort(netip.MustParseAddrPort("10.0.0.2:19082"))
	if err != nil {
		t.Fatal(err)
	}
	udp, err := stack.ListenUDPAddrPort(netip.MustParseAddrPort("10.0.0.2:19082"))
	if err != nil {
		t.Fatal(err)
	}
	c := &client{stack: stack, tun: netip.MustParseAddr("10.0.0.2")}
	s := &server{tcp: tcp, udp: udp, done: make(chan struct{})}
	go c.tcpEcho(s, tcp)
	go c.udpEcho(s, udp)
	defer s.close()
	mux := httpMux(c, "secret")
	call := func(path, body string) *httptest.ResponseRecorder {
		r := httptest.NewRequest("POST", path, strings.NewReader(body))
		r.Header.Set("Authorization", "Bearer secret")
		r.Header.Set("Content-Type", "application/json")
		w := httptest.NewRecorder()
		mux.ServeHTTP(w, r)
		if w.Code != 200 {
			t.Fatalf("%s returned %d: %s", path, w.Code, w.Body.String())
		}
		return w
	}
	opened := call("/tcp/open", `{"target":"10.0.0.2:19082"}`)
	var open tcpOpenResponse
	if err := json.Unmarshal(opened.Body.Bytes(), &open); err != nil || open.ID == "" || open.Error != "" {
		t.Fatalf("open response %s, err=%v", opened.Body.String(), err)
	}
	exchanged := call("/tcp/exchange", fmt.Sprintf(`{"id":%q,"payload":"same-connection","timeout_ms":1000}`, open.ID))
	var reply probeResponse
	if err := json.Unmarshal(exchanged.Body.Bytes(), &reply); err != nil || !reply.OK {
		t.Fatalf("exchange response %s, err=%v", exchanged.Body.String(), err)
	}
	call("/tcp/close", fmt.Sprintf(`{"id":%q}`, open.ID))
	if _, exists := c.tcpConns[open.ID]; exists {
		t.Fatal("close left persistent connection registered")
	}
}

func TestConfigureClosesPersistentTCPConnections(t *testing.T) {
	clientSide, peerSide := net.Pipe()
	defer peerSide.Close()
	c := &client{tcpConns: map[string]net.Conn{"existing": clientSide}}
	mux := httpMux(c, "secret")
	r := httptest.NewRequest("POST", "/configure", strings.NewReader(fmt.Sprintf(`{"config":%q}`, validConfig())))
	r.Header.Set("Authorization", "Bearer secret")
	w := httptest.NewRecorder()
	mux.ServeHTTP(w, r)
	if w.Code != 200 {
		t.Fatalf("configure returned %d: %s", w.Code, w.Body.String())
	}
	defer c.dev.Close()
	if _, err := peerSide.Write([]byte("after replacement")); err == nil {
		t.Fatal("device replacement left persistent connection open")
	}
	if len(c.tcpConns) != 0 {
		t.Fatalf("device replacement retained %d persistent connections", len(c.tcpConns))
	}
}
