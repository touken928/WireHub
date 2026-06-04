package l4

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/netip"
	"sync"
	"time"

	"github.com/touken928/wirehub/internal/vpn/stackutil"
	"golang.zx2c4.com/wireguard/tun/netstack"
	"gvisor.dev/gvisor/pkg/tcpip"
	"gvisor.dev/gvisor/pkg/tcpip/adapters/gonet"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv4"
	"gvisor.dev/gvisor/pkg/tcpip/stack"
	"gvisor.dev/gvisor/pkg/tcpip/transport/tcp"
	"gvisor.dev/gvisor/pkg/tcpip/transport/udp"
	"gvisor.dev/gvisor/pkg/waiter"
)

const mapNIC tcpip.NICID = 1

// MapRule is a runtime virtual-IP map mapping (TCP/UDP, port-preserving).
type MapRule struct {
	ID          uint
	Slug        string
	TargetHost  string
	VirtualIP   netip.Addr
	AllowedPeer func(peerWGIP netip.Addr) bool
}

type udpMapSession struct {
	client     *gonet.UDPConn
	backend    net.Conn
	lastActive time.Time
}

// MapProxy terminates TCP/UDP to map virtual IPs and dials map targets.
type MapProxy struct {
	tnet      *netstack.Net
	vpnSubnet *net.IPNet
	resolver  HostResolver

	mu          sync.Mutex
	rules       map[netip.Addr]*MapRule
	slugByIP    map[netip.Addr]string
	cancel      context.CancelFunc
	tcpFwd      *tcp.Forwarder
	udpFwd      *udp.Forwarder
	udpMu       sync.Mutex
	udpSessions map[flowKey]*udpMapSession
}

func NewMapProxy(tnet *netstack.Net, vpnSubnet string, resolver HostResolver) (*MapProxy, error) {
	subnet, err := parseVPNSubnet(vpnSubnet)
	if err != nil {
		return nil, err
	}
	return &MapProxy{
		tnet:        tnet,
		vpnSubnet:   subnet,
		resolver:    resolver,
		rules:       make(map[netip.Addr]*MapRule),
		slugByIP:    make(map[netip.Addr]string),
		udpSessions: make(map[flowKey]*udpMapSession),
	}, nil
}

func (m *MapProxy) Apply(rules []MapRule) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if m.cancel != nil {
		m.cancel()
		m.cancel = nil
	}
	m.closeUDPSessions()
	m.rules = make(map[netip.Addr]*MapRule, len(rules))
	m.slugByIP = make(map[netip.Addr]string, len(rules))

	stk, err := stackutil.StackFromNet(m.tnet)
	if err != nil {
		return err
	}

	for i := range rules {
		rule := rules[i]
		if err := ensureMapAddress(stk, rule.VirtualIP); err != nil {
			log.Printf("map %s: add address %s: %v", rule.Slug, rule.VirtualIP, err)
			continue
		}
		m.rules[rule.VirtualIP] = &rules[i]
		m.slugByIP[rule.VirtualIP] = rule.Slug
	}

	ctx, cancel := context.WithCancel(context.Background())
	m.cancel = cancel

	if m.tcpFwd == nil {
		m.tcpFwd = tcp.NewForwarder(stk, 0, 512, func(req *tcp.ForwarderRequest) {
			m.handleTCPForwarderRequest(ctx, req)
		})
		stk.SetTransportProtocolHandler(tcp.ProtocolNumber, m.tcpFwd.HandlePacket)
	}
	if m.udpFwd == nil {
		m.udpFwd = udp.NewForwarder(stk, func(req *udp.ForwarderRequest) {
			m.handleUDPForwarderRequest(ctx, req)
		})
		stk.SetTransportProtocolHandler(udp.ProtocolNumber, m.udpFwd.HandlePacket)
	}

	return nil
}

func (m *MapProxy) Stop() {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.cancel != nil {
		m.cancel()
		m.cancel = nil
	}
	m.closeUDPSessions()
	m.rules = make(map[netip.Addr]*MapRule)
	m.slugByIP = make(map[netip.Addr]string)
}

func (m *MapProxy) closeUDPSessions() {
	m.udpMu.Lock()
	defer m.udpMu.Unlock()
	for _, sess := range m.udpSessions {
		_ = sess.backend.Close()
		_ = sess.client.Close()
	}
	m.udpSessions = make(map[flowKey]*udpMapSession)
}

func ensureMapAddress(stk *stack.Stack, addr netip.Addr) error {
	if !addr.IsValid() || !addr.Is4() {
		return nil
	}
	protoAddr := tcpip.ProtocolAddress{
		Protocol:          ipv4.ProtocolNumber,
		AddressWithPrefix: tcpip.AddrFromSlice(addr.AsSlice()).WithPrefix(),
	}
	if err := stk.AddProtocolAddress(mapNIC, protoAddr, stack.AddressProperties{}); err != nil {
		if _, ok := err.(*tcpip.ErrDuplicateAddress); ok {
			return nil
		}
		return fmt.Errorf("%v", err)
	}
	return nil
}

func (m *MapProxy) handleTCPForwarderRequest(ctx context.Context, req *tcp.ForwarderRequest) {
	id := req.ID()
	localIP, ok := netip.AddrFromSlice(id.LocalAddress.AsSlice())
	if !ok {
		return
	}

	m.mu.Lock()
	rule := m.rules[localIP]
	m.mu.Unlock()
	if rule == nil {
		req.Complete(true)
		return
	}

	remoteIP, ok := netip.AddrFromSlice(id.RemoteAddress.AsSlice())
	if !ok {
		req.Complete(true)
		return
	}
	if rule.AllowedPeer != nil && !rule.AllowedPeer(remoteIP) {
		req.Complete(true)
		return
	}

	var wq waiter.Queue
	ep, tcpErr := req.CreateEndpoint(&wq)
	if tcpErr != nil {
		req.Complete(true)
		return
	}
	req.Complete(false)

	go m.proxyTCP(ctx, rule, gonet.NewTCPConn(&wq, ep), id.LocalPort)
}

func (m *MapProxy) handleUDPForwarderRequest(ctx context.Context, req *udp.ForwarderRequest) {
	id := req.ID()
	localIP, ok := netip.AddrFromSlice(id.LocalAddress.AsSlice())
	if !ok {
		return
	}

	m.mu.Lock()
	rule := m.rules[localIP]
	m.mu.Unlock()
	if rule == nil {
		return
	}

	remoteIP, ok := netip.AddrFromSlice(id.RemoteAddress.AsSlice())
	if !ok {
		return
	}
	if rule.AllowedPeer != nil && !rule.AllowedPeer(remoteIP) {
		return
	}

	key := flowKey{
		client:     remoteIP,
		server:     localIP,
		clientPort: id.RemotePort,
		serverPort: id.LocalPort,
		proto:      protoUDP,
	}

	m.udpMu.Lock()
	if _, exists := m.udpSessions[key]; exists {
		m.udpMu.Unlock()
		return
	}
	m.udpMu.Unlock()

	var wq waiter.Queue
	ep, udpErr := req.CreateEndpoint(&wq)
	if udpErr != nil {
		return
	}
	client := gonet.NewUDPConn(&wq, ep)

	addrs, err := m.resolver.ResolveForwardAddrs(rule.TargetHost)
	if err != nil {
		log.Printf("map %s resolve %q: %v", rule.Slug, rule.TargetHost, err)
		_ = client.Close()
		return
	}
	var backend net.Conn
	for _, addr := range addrs {
		target := netip.AddrPortFrom(addr, id.LocalPort)
		backend, err = m.dialTarget(ctx, "udp", target)
		if err == nil {
			break
		}
		log.Printf("map %s dial %s: %v", rule.Slug, target, err)
	}
	if backend == nil {
		_ = client.Close()
		return
	}

	sess := &udpMapSession{
		client:     client,
		backend:    backend,
		lastActive: time.Now(),
	}
	m.udpMu.Lock()
	if _, exists := m.udpSessions[key]; exists {
		m.udpMu.Unlock()
		_ = backend.Close()
		_ = client.Close()
		return
	}
	m.udpSessions[key] = sess
	m.udpMu.Unlock()

	go m.mapUDPClientToBackend(ctx, rule, sess)
	go m.mapUDPBackendToClient(ctx, key, sess)
}

func (m *MapProxy) mapUDPClientToBackend(ctx context.Context, rule *MapRule, sess *udpMapSession) {
	buf := make([]byte, 64*1024)
	for {
		if ctx.Err() != nil {
			return
		}
		_ = sess.client.SetReadDeadline(time.Now().Add(SessionIdle))
		n, err := sess.client.Read(buf)
		if err != nil {
			return
		}
		if _, err := sess.backend.Write(buf[:n]); err != nil {
			log.Printf("map %s client->backend: %v", rule.Slug, err)
			return
		}
		m.udpMu.Lock()
		sess.lastActive = time.Now()
		m.udpMu.Unlock()
	}
}

func (m *MapProxy) mapUDPBackendToClient(ctx context.Context, key flowKey, sess *udpMapSession) {
	defer func() {
		_ = sess.backend.Close()
		_ = sess.client.Close()
		m.udpMu.Lock()
		delete(m.udpSessions, key)
		m.udpMu.Unlock()
	}()
	buf := make([]byte, 64*1024)
	for {
		if ctx.Err() != nil {
			return
		}
		_ = sess.backend.SetReadDeadline(time.Now().Add(SessionIdle))
		n, err := sess.backend.Read(buf)
		if err != nil {
			return
		}
		if _, err := sess.client.Write(buf[:n]); err != nil {
			return
		}
		m.udpMu.Lock()
		sess.lastActive = time.Now()
		m.udpMu.Unlock()
	}
}

func (m *MapProxy) proxyTCP(ctx context.Context, rule *MapRule, client *gonet.TCPConn, localPort uint16) {
	defer client.Close()

	addrs, err := m.resolver.ResolveForwardAddrs(rule.TargetHost)
	if err != nil {
		log.Printf("map %s resolve %q: %v", rule.Slug, rule.TargetHost, err)
		return
	}
	var remote net.Conn
	for _, addr := range addrs {
		target := netip.AddrPortFrom(addr, localPort)
		remote, err = m.dialTarget(ctx, "tcp", target)
		if err == nil {
			break
		}
		log.Printf("map %s dial %s: %v", rule.Slug, target, err)
	}
	if remote == nil {
		return
	}
	defer remote.Close()

	done := make(chan struct{}, 2)
	go func() {
		_, _ = io.Copy(remote, client)
		done <- struct{}{}
	}()
	go func() {
		_, _ = io.Copy(client, remote)
		done <- struct{}{}
	}()
	select {
	case <-ctx.Done():
	case <-done:
		_ = remote.Close()
		<-done
	}
}

func (m *MapProxy) dialTarget(ctx context.Context, network string, target netip.AddrPort) (net.Conn, error) {
	fp := &ForwardProxy{
		tnet:      m.tnet,
		vpnSubnet: m.vpnSubnet,
		resolver:  m.resolver,
	}
	return fp.dialTarget(ctx, network, target)
}

// MapVIPAddrs collects virtual IPs from map rules.
func MapVIPAddrs(rules []MapRule) []netip.Addr {
	out := make([]netip.Addr, 0, len(rules))
	for _, r := range rules {
		if r.VirtualIP.IsValid() {
			out = append(out, r.VirtualIP)
		}
	}
	return out
}

// ParseMapVIP parses map virtual IP strings for stack startup.
func ParseMapVIP(ips []string) []netip.Addr {
	out := make([]netip.Addr, 0, len(ips))
	for _, s := range ips {
		addr, err := netip.ParseAddr(s)
		if err == nil && addr.Is4() {
			out = append(out, addr)
		}
	}
	return out
}
