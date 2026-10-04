package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/netip"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	"golang.org/x/net/icmp"
	"golang.org/x/net/ipv4"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

const (
	maxBody    = 64 << 10
	maxPayload = 4096
	maxEvents  = 256
	maxTimeout = 5 * time.Second
)

type event struct {
	Protocol string `json:"protocol"`
	Payload  string `json:"payload"`
}
type server struct {
	tcp  net.Listener
	udp  net.PacketConn
	done chan struct{}
}
type client struct {
	mu       sync.Mutex // Serializes configure with every operation that uses a device or netstack.
	dev      *device.Device
	tun      netip.Addr
	stack    *netstack.Net
	servers  []*server
	tcpConns map[string]net.Conn
	eventsMu sync.Mutex
	events   []event
}

type configureRequest struct {
	Config string `json:"config"`
}
type serveRequest struct {
	Port int `json:"port"`
}
type probeRequest struct {
	Protocol  string `json:"protocol"`
	Target    string `json:"target"`
	Payload   string `json:"payload"`
	TimeoutMS int    `json:"timeout_ms"`
}
type probeResponse struct {
	OK    bool   `json:"ok"`
	Error string `json:"error,omitempty"`
}
type tcpOpenRequest struct {
	Target string `json:"target"`
}
type tcpExchangeRequest struct {
	ID        string `json:"id"`
	Payload   string `json:"payload"`
	TimeoutMS int    `json:"timeout_ms"`
}
type tcpOpenResponse struct {
	ID    string `json:"id,omitempty"`
	Error string `json:"error,omitempty"`
}

func main() {
	listen := flag.String("listen", "127.0.0.1:8080", "control HTTP listen address")
	readyFile := flag.String("ready-file", "", "write bound control address after listening")
	flag.Parse()
	token := os.Getenv("TEST_CONTROL_TOKEN")
	if token == "" {
		log.Fatal("TEST_CONTROL_TOKEN is required")
	}
	c := &client{}
	mux := httpMux(c, token)
	s := &http.Server{Addr: *listen, Handler: mux, ReadHeaderTimeout: 3 * time.Second, ReadTimeout: 10 * time.Second, WriteTimeout: 65 * time.Second, IdleTimeout: 30 * time.Second}
	listener, err := net.Listen("tcp", *listen)
	if err != nil {
		log.Fatal("control listener failed")
	}
	if *readyFile != "" {
		if err := os.WriteFile(*readyFile, []byte(listener.Addr().String()), 0600); err != nil {
			listener.Close()
			log.Fatal("control readiness file failed")
		}
	}
	log.Fatal(s.Serve(listener))
}

func httpMux(c *client, token string) http.Handler {
	m := http.NewServeMux()
	m.HandleFunc("GET /health", func(w http.ResponseWriter, r *http.Request) { jsonReply(w, http.StatusOK, map[string]bool{"ok": true}) })
	m.HandleFunc("POST /configure", c.configureHTTP)
	m.HandleFunc("POST /serve", c.serveHTTP)
	m.HandleFunc("POST /probe", c.probeHTTP)
	m.HandleFunc("POST /tcp/open", c.tcpOpenHTTP)
	m.HandleFunc("POST /tcp/exchange", c.tcpExchangeHTTP)
	m.HandleFunc("POST /tcp/transfer", c.tcpTransferHTTP)
	m.HandleFunc("POST /tcp/close", c.tcpCloseHTTP)
	m.HandleFunc("GET /status", c.statusHTTP)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/health" && r.Method == http.MethodGet {
			m.ServeHTTP(w, r)
			return
		}
		if r.Header.Get("Authorization") != "Bearer "+token {
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}
		m.ServeHTTP(w, r)
	})
}

// Imports are kept explicit rather than relying on a framework; body limits are enforced before decoding.
func decodeBody(w http.ResponseWriter, r *http.Request, v any) bool {
	r.Body = http.MaxBytesReader(w, r.Body, maxBody)
	d := json.NewDecoder(r.Body)
	d.DisallowUnknownFields()
	if err := d.Decode(v); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return false
	}
	var extra any
	if err := d.Decode(&extra); err != io.EOF {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return false
	}
	return true
}
func jsonReply(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}
func (c *client) configureHTTP(w http.ResponseWriter, r *http.Request) {
	var req configureRequest
	if !decodeBody(w, r, &req) {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	tunIP, stack, dev, err := newDevice(req.Config)
	if err != nil {
		http.Error(w, "invalid client configuration", http.StatusBadRequest)
		return
	}
	for _, s := range c.servers {
		s.close()
	}
	c.servers = nil
	for id, conn := range c.tcpConns {
		_ = conn.Close()
		delete(c.tcpConns, id)
	}
	if c.dev != nil {
		c.dev.Close()
	}
	c.dev, c.tun, c.stack = dev, tunIP, stack
	c.eventsMu.Lock()
	c.events = nil
	c.eventsMu.Unlock()
	jsonReply(w, http.StatusOK, map[string]bool{"ok": true})
}

type parsedConfig struct {
	private, peer, endpoint, address, allowed string
	mtu, keepalive                            int
}

func parseConfig(text string) (parsedConfig, error) {
	out := parsedConfig{mtu: 1420}
	if len(text) == 0 || len(text) > maxBody || strings.ContainsRune(text, '\x00') {
		return out, errors.New("invalid config")
	}
	section := ""
	seen := map[string]bool{}
	for _, raw := range strings.Split(text, "\n") {
		line := strings.TrimSpace(strings.TrimSuffix(raw, "\r"))
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, ";") {
			continue
		}
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			section = strings.ToLower(strings.TrimSpace(line[1 : len(line)-1]))
			if section != "interface" && section != "peer" {
				return out, errors.New("bad section")
			}
			continue
		}
		if section == "" {
			return out, errors.New("field outside section")
		}
		k, v, ok := strings.Cut(line, "=")
		if !ok {
			return out, errors.New("bad field")
		}
		k = strings.ToLower(strings.TrimSpace(k))
		v = strings.TrimSpace(v)
		if strings.ContainsAny(v, "\r\n") || v == "" {
			return out, errors.New("bad value")
		}
		key := section + "." + k
		if seen[key] {
			return out, errors.New("duplicate field")
		}
		seen[key] = true
		switch key {
		case "interface.privatekey":
			out.private = v
		case "interface.address":
			out.address = v
		case "interface.mtu":
			value, err := strconv.Atoi(v)
			if err != nil || value < 576 || value > 1420 {
				return out, errors.New("bad MTU")
			}
			out.mtu = value
		case "peer.persistentkeepalive":
			value, err := strconv.Atoi(v)
			if err != nil || value < 0 || value > 65535 {
				return out, errors.New("bad keepalive")
			}
			out.keepalive = value
		case "interface.dns", "interface.listenport": // not needed by the isolated netstack
		case "peer.publickey":
			out.peer = v
		case "peer.endpoint":
			out.endpoint = v
		case "peer.allowedips":
			out.allowed = v
		default:
			return out, errors.New("unknown field")
		}
	}
	priv, e1 := decodeKey(out.private)
	peer, e2 := decodeKey(out.peer)
	if e1 != nil || e2 != nil {
		return out, errors.New("bad key")
	}
	_ = priv
	_ = peer
	addr, err := netip.ParsePrefix(out.address)
	if err != nil || !addr.Addr().Is4() || addr.Bits() != 32 {
		return out, errors.New("bad address")
	}
	allowed, err := netip.ParsePrefix(out.allowed)
	if err != nil || !allowed.Addr().Is4() {
		return out, errors.New("bad allowed IP")
	}
	if strings.ContainsAny(out.endpoint, "\r\n") || out.endpoint == "" {
		return out, errors.New("bad endpoint")
	}
	if _, err := net.ResolveUDPAddr("udp", out.endpoint); err != nil {
		return out, errors.New("bad endpoint")
	}
	return out, nil
}
func decodeKey(s string) ([]byte, error) {
	b, e := base64.StdEncoding.DecodeString(s)
	if e != nil || len(b) != 32 {
		return nil, errors.New("bad key")
	}
	return b, nil
}
func newDevice(config string) (netip.Addr, *netstack.Net, *device.Device, error) {
	p, err := parseConfig(config)
	if err != nil {
		return netip.Addr{}, nil, nil, err
	}
	priv, _ := decodeKey(p.private)
	peer, _ := decodeKey(p.peer)
	addr, _ := netip.ParsePrefix(p.address)
	allowed, _ := netip.ParsePrefix(p.allowed)
	outer, err := net.ResolveUDPAddr("udp", p.endpoint)
	if err != nil {
		return netip.Addr{}, nil, nil, err
	}
	tun, stack, err := netstack.CreateNetTUN([]netip.Addr{addr.Addr()}, nil, p.mtu)
	if err != nil {
		return netip.Addr{}, nil, nil, err
	}
	dev := device.NewDevice(tun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelSilent, ""))
	ipc := fmt.Sprintf("private_key=%s\npublic_key=%s\nendpoint=%s\nallowed_ip=%s\npersistent_keepalive_interval=%d\n", hex.EncodeToString(priv), hex.EncodeToString(peer), outer.String(), allowed.String(), p.keepalive)
	err = dev.IpcSet(ipc)
	if err == nil {
		err = dev.Up()
	}
	if err != nil {
		dev.Close()
		return netip.Addr{}, nil, nil, errors.New("device setup failed")
	}
	return addr.Addr(), stack, dev, nil
}

func (c *client) serveHTTP(w http.ResponseWriter, r *http.Request) {
	var q serveRequest
	if !decodeBody(w, r, &q) {
		return
	}
	if q.Port < 1 || q.Port > 65535 {
		http.Error(w, "invalid port", 400)
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.stack == nil {
		http.Error(w, "not configured", http.StatusConflict)
		return
	}
	addr := netip.AddrPortFrom(c.tun, uint16(q.Port))
	tcp, err := c.stack.ListenTCPAddrPort(addr)
	if err != nil {
		http.Error(w, "listen failed", 500)
		return
	}
	udp, err := c.stack.ListenUDPAddrPort(addr)
	if err != nil {
		tcp.Close()
		http.Error(w, "listen failed", 500)
		return
	}
	s := &server{tcp: tcp, udp: udp, done: make(chan struct{})}
	c.servers = append(c.servers, s)
	go c.tcpEcho(s, tcp)
	go c.udpEcho(s, udp)
	jsonReply(w, 200, map[string]bool{"ok": true})
}
func (c *client) record(proto string, p []byte) {
	c.eventsMu.Lock()
	defer c.eventsMu.Unlock()
	c.events = append(c.events, event{proto, string(bytes.Clone(p))})
	if len(c.events) > maxEvents {
		c.events = append([]event(nil), c.events[len(c.events)-maxEvents:]...)
	}
}
func (c *client) tcpEcho(s *server, l net.Listener) {
	sem := make(chan struct{}, 64)
	for {
		conn, err := l.Accept()
		if err != nil {
			return
		}
		select {
		case sem <- struct{}{}:
			go func(n net.Conn) {
				defer func() { <-sem; n.Close() }()
				buf := make([]byte, maxPayload)
				for {
					_ = n.SetDeadline(time.Now().Add(5 * time.Minute))
					nr, e := n.Read(buf)
					if nr > 0 {
						p := bytes.Clone(buf[:nr])
						c.record("tcp", p)
						if _, we := n.Write(p); we != nil {
							return
						}
					}
					if e != nil {
						return
					}
				}
			}(conn)
		default:
			conn.Close()
		}
	}
}
func (c *client) udpEcho(s *server, conn net.PacketConn) {
	buf := make([]byte, maxPayload)
	for {
		_ = conn.SetReadDeadline(time.Now().Add(time.Second))
		n, a, err := conn.ReadFrom(buf)
		if err != nil {
			select {
			case <-s.done:
				return
			default:
			}
			if ne, ok := err.(net.Error); ok && ne.Timeout() {
				continue
			}
			return
		}
		p := bytes.Clone(buf[:n])
		c.record("udp", p)
		_, _ = conn.WriteTo(p, a)
	}
}
func (s *server) close() { close(s.done); _ = s.tcp.Close(); _ = s.udp.Close() }

func (c *client) probeHTTP(w http.ResponseWriter, r *http.Request) {
	var q probeRequest
	if !decodeBody(w, r, &q) {
		return
	}
	if q.Protocol != "tcp" && q.Protocol != "udp" && q.Protocol != "icmp" {
		http.Error(w, "invalid protocol", 400)
		return
	}
	if len(q.Payload) > maxPayload || q.TimeoutMS < 1 || time.Duration(q.TimeoutMS)*time.Millisecond > maxTimeout {
		http.Error(w, "invalid probe limits", 400)
		return
	}
	var target netip.AddrPort
	if q.Protocol == "icmp" {
		ip, err := netip.ParseAddr(q.Target)
		if err != nil || !ip.Is4() {
			http.Error(w, "invalid target", 400)
			return
		}
		target = netip.AddrPortFrom(ip, 0)
	} else {
		var err error
		target, err = netip.ParseAddrPort(q.Target)
		if err != nil || !target.Addr().Is4() || target.Port() == 0 {
			http.Error(w, "invalid target", 400)
			return
		}
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.stack == nil {
		jsonReply(w, 200, probeResponse{Error: "not configured"})
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), time.Duration(q.TimeoutMS)*time.Millisecond)
	defer cancel()
	var reply []byte
	var err error
	switch q.Protocol {
	case "tcp":
		conn, e := c.stack.DialContextTCPAddrPort(ctx, target)
		if e != nil {
			err = e
			break
		}
		defer conn.Close()
		_ = conn.SetDeadline(deadline(ctx))
		_, e = conn.Write([]byte(q.Payload))
		if e == nil {
			reply = make([]byte, len(q.Payload))
			_, e = io.ReadFull(conn, reply)
		}
		err = e
	case "udp":
		conn, e := c.stack.DialUDPAddrPort(netip.AddrPort{}, target)
		if e != nil {
			err = e
			break
		}
		defer conn.Close()
		_ = conn.SetDeadline(deadline(ctx))
		_, e = conn.Write([]byte(q.Payload))
		if e == nil {
			reply = make([]byte, maxPayload)
			var n int
			n, e = conn.Read(reply)
			reply = reply[:n]
		}
		err = e
	case "icmp":
		conn, e := c.stack.DialPingAddr(netip.Addr{}, target.Addr())
		if e != nil {
			err = e
			break
		}
		defer conn.Close()
		_ = conn.SetDeadline(deadline(ctx))
		m := &icmp.Message{Type: ipv4.ICMPTypeEcho, Body: &icmp.Echo{ID: 1, Seq: 1, Data: []byte(q.Payload)}}
		packet, e := m.Marshal(nil)
		if e == nil {
			_, e = conn.Write(packet)
		}
		if e == nil {
			buf := make([]byte, 1500)
			var n int
			n, e = conn.Read(buf)
			if e == nil {
				var msg *icmp.Message
				msg, e = icmp.ParseMessage(1, buf[:n])
				if e == nil {
					echo, ok := msg.Body.(*icmp.Echo)
					if !ok || msg.Type != ipv4.ICMPTypeEchoReply || !bytes.Equal(echo.Data, []byte(q.Payload)) {
						e = errors.New("invalid echo reply")
					} else {
						reply = bytes.Clone(echo.Data)
					}
				}
			}
		}
		err = e
	}
	if err != nil {
		jsonReply(w, 200, probeResponse{Error: "probe failed"})
		return
	}
	jsonReply(w, 200, probeResponse{OK: bytes.Equal(reply, []byte(q.Payload))})
}

func (c *client) tcpOpenHTTP(w http.ResponseWriter, r *http.Request) {
	var q tcpOpenRequest
	if !decodeBody(w, r, &q) {
		return
	}
	target, err := netip.ParseAddrPort(q.Target)
	if err != nil || !target.Addr().Is4() || target.Port() == 0 {
		http.Error(w, "invalid target", http.StatusBadRequest)
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.stack == nil {
		jsonReply(w, http.StatusOK, tcpOpenResponse{Error: "not configured"})
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), maxTimeout)
	defer cancel()
	conn, err := c.stack.DialContextTCPAddrPort(ctx, target)
	if err != nil {
		jsonReply(w, http.StatusOK, tcpOpenResponse{Error: "connect failed"})
		return
	}
	var random [16]byte
	// The connection handle is an unguessable, process-local identifier and is
	// only usable through this client's authenticated control API.
	if _, err := rand.Read(random[:]); err != nil {
		_ = conn.Close()
		jsonReply(w, http.StatusOK, tcpOpenResponse{Error: "connect failed"})
		return
	}
	id := hex.EncodeToString(random[:])
	if c.tcpConns == nil {
		c.tcpConns = make(map[string]net.Conn)
	}
	c.tcpConns[id] = conn
	jsonReply(w, http.StatusOK, tcpOpenResponse{ID: id})
}

func (c *client) tcpExchangeHTTP(w http.ResponseWriter, r *http.Request) {
	var q tcpExchangeRequest
	if !decodeBody(w, r, &q) {
		return
	}
	if q.ID == "" || len(q.Payload) > maxPayload || q.TimeoutMS < 1 || time.Duration(q.TimeoutMS)*time.Millisecond > maxTimeout {
		http.Error(w, "invalid exchange limits", http.StatusBadRequest)
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	conn := c.tcpConns[q.ID]
	if conn == nil {
		jsonReply(w, http.StatusOK, probeResponse{Error: "connection unavailable"})
		return
	}
	_ = conn.SetDeadline(time.Now().Add(time.Duration(q.TimeoutMS) * time.Millisecond))
	_, err := conn.Write([]byte(q.Payload))
	var reply []byte
	if err == nil {
		reply = make([]byte, len(q.Payload))
		_, err = io.ReadFull(conn, reply)
	}
	if err != nil {
		jsonReply(w, http.StatusOK, probeResponse{Error: "exchange failed"})
		return
	}
	jsonReply(w, http.StatusOK, probeResponse{OK: bytes.Equal(reply, []byte(q.Payload))})
}

// Transfer bytes on an existing socket, verifying the entire echo without logging payloads.
func (c *client) tcpTransferHTTP(w http.ResponseWriter, r *http.Request) {
	var q struct {
		ID        string `json:"id"`
		Bytes     int    `json:"bytes"`
		TimeoutMS int    `json:"timeout_ms"`
	}
	if !decodeBody(w, r, &q) {
		return
	}
	if q.ID == "" || q.Bytes < 1 || q.Bytes > 16<<20 || q.TimeoutMS < 1 || q.TimeoutMS > 60000 {
		http.Error(w, "invalid transfer limits", 400)
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	conn := c.tcpConns[q.ID]
	if conn == nil {
		jsonReply(w, 200, probeResponse{Error: "connection unavailable"})
		return
	}
	payload := make([]byte, q.Bytes)
	for i := range payload {
		payload[i] = byte(i*31 + 17)
	}
	expected := sha256.Sum256(payload)
	_ = conn.SetDeadline(time.Now().Add(time.Duration(q.TimeoutMS) * time.Millisecond))
	started := time.Now()
	written := make(chan error, 1)
	go func() { _, err := io.Copy(conn, bytes.NewReader(payload)); written <- err }()
	hash := sha256.New()
	received, err := io.CopyN(hash, conn, int64(q.Bytes))
	if err != nil {
		_ = conn.SetDeadline(time.Now())
	}
	writeErr := <-written
	duration := time.Since(started)
	ok := err == nil && writeErr == nil && received == int64(q.Bytes) && bytes.Equal(hash.Sum(nil), expected[:])
	jsonReply(w, 200, map[string]any{"ok": ok, "bytes": received, "seconds": duration.Seconds(), "sha256": hex.EncodeToString(hash.Sum(nil))})
}

func (c *client) tcpCloseHTTP(w http.ResponseWriter, r *http.Request) {
	var q struct {
		ID string `json:"id"`
	}
	if !decodeBody(w, r, &q) {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if conn := c.tcpConns[q.ID]; conn != nil {
		_ = conn.Close()
		delete(c.tcpConns, q.ID)
	}
	jsonReply(w, http.StatusOK, map[string]bool{"ok": true})
}
func deadline(ctx context.Context) time.Time { d, _ := ctx.Deadline(); return d }
func (c *client) statusHTTP(w http.ResponseWriter, r *http.Request) {
	c.mu.Lock()
	defer c.mu.Unlock()
	handshake := false
	if c.dev != nil {
		if got, err := c.dev.IpcGet(); err == nil {
			var sec, nsec int64
			for _, line := range strings.Split(got, "\n") {
				k, v, ok := strings.Cut(line, "=")
				if !ok {
					continue
				}
				n, e := strconv.ParseInt(v, 10, 64)
				if e != nil {
					continue
				}
				if k == "last_handshake_time_sec" {
					sec = n
				}
				if k == "last_handshake_time_nsec" {
					nsec = n
				}
			}
			handshake = sec > 0 || nsec > 0
		}
	}
	c.eventsMu.Lock()
	received := append([]event(nil), c.events...)
	c.eventsMu.Unlock()
	jsonReply(w, 200, map[string]any{"handshake": handshake, "received": received})
}
