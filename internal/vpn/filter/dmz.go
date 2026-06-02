package filter

import (
	"context"
	"io"
	"log"
	"net/netip"
	"sync"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
	"gvisor.dev/gvisor/pkg/tcpip/adapters/gonet"
	"gvisor.dev/gvisor/pkg/tcpip/stack"
	"gvisor.dev/gvisor/pkg/tcpip/transport/tcp"
	"gvisor.dev/gvisor/pkg/tcpip/transport/udp"
	"gvisor.dev/gvisor/pkg/waiter"
)

// DMZConfig is the runtime DMZ target (same listen port on hub → same port on target).
type DMZConfig struct {
	Enabled    bool
	TargetHost string
}

type dmzManager struct {
	tnet     *netstack.Net
	resolver HostResolver
	hubIP    netip.Addr

	mu         sync.RWMutex
	enabled    bool
	targetHost string
	tcpClaimed map[uint16]struct{}
	udpClaimed map[uint16]struct{}

	installOnce sync.Once
	tcpForwarder *tcp.Forwarder
}

func newDMZManager(tnet *netstack.Net, hubIP netip.Addr, resolver HostResolver) *dmzManager {
	return &dmzManager{
		tnet:       tnet,
		resolver:   resolver,
		hubIP:      hubIP,
		tcpClaimed: make(map[uint16]struct{}),
		udpClaimed: make(map[uint16]struct{}),
	}
}

func (m *dmzManager) configure(dmz DMZConfig, rules []PortForwardRule, hubWebPort int) {
	tcpClaimed, udpClaimed := buildClaimedListenPorts(hubWebPort, rules)

	m.mu.Lock()
	m.enabled = dmz.Enabled && dmz.TargetHost != ""
	m.targetHost = dmz.TargetHost
	m.tcpClaimed = tcpClaimed
	m.udpClaimed = udpClaimed
	m.mu.Unlock()
}

func (m *dmzManager) install(stk *stack.Stack) {
	m.installOnce.Do(func() {
		m.tcpForwarder = tcp.NewForwarder(stk, 0, 4096, m.handleTCPForward)
		stk.SetTransportProtocolHandler(tcp.ProtocolNumber, func(id stack.TransportEndpointID, pkt *stack.PacketBuffer) bool {
			if !m.shouldHandle("tcp", id) {
				return false
			}
			return m.tcpForwarder.HandlePacket(id, pkt)
		})

		udpFwd := udp.NewForwarder(stk, m.handleUDPForward)
		stk.SetTransportProtocolHandler(udp.ProtocolNumber, func(id stack.TransportEndpointID, pkt *stack.PacketBuffer) bool {
			if !m.shouldHandle("udp", id) {
				return false
			}
			return udpFwd.HandlePacket(id, pkt)
		})
		log.Printf("port forward DMZ handlers installed on hub %s", m.hubIP)
	})
}

func (m *dmzManager) shouldHandle(proto string, id stack.TransportEndpointID) bool {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if !m.enabled {
		return false
	}
	local, ok := netip.AddrFromSlice(id.LocalAddress.AsSlice())
	if !ok || local != m.hubIP {
		return false
	}
	port := id.LocalPort
	switch proto {
	case "tcp":
		_, claimed := m.tcpClaimed[port]
		return !claimed
	case "udp":
		_, claimed := m.udpClaimed[port]
		return !claimed
	default:
		return false
	}
}

func (m *dmzManager) target() (host string, ok bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if !m.enabled || m.targetHost == "" {
		return "", false
	}
	return m.targetHost, true
}

func (m *dmzManager) handleTCPForward(req *tcp.ForwarderRequest) {
	id := req.ID()
	port := id.LocalPort
	targetHost, ok := m.target()
	if !ok {
		req.Complete(true)
		return
	}

	go func() {
		var wq waiter.Queue
		ep, err := req.CreateEndpoint(&wq)
		if err != nil {
			req.Complete(true)
			return
		}
		defer req.Complete(false)

		client := gonet.NewTCPConn(&wq, ep)
		defer client.Close()

		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()

		addr, resolveErr := m.resolver.ResolveHost(targetHost)
		if resolveErr != nil {
			log.Printf("dmz tcp resolve %q: %v", targetHost, resolveErr)
			return
		}
		target := netip.AddrPortFrom(addr, port)
		remote, dialErr := m.tnet.DialContext(ctx, "tcp", target.String())
		if dialErr != nil {
			log.Printf("dmz tcp dial %s: %v", target, dialErr)
			return
		}
		defer remote.Close()

		log.Printf("dmz tcp %s:%d -> %s", m.hubIP, port, target)

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
	}()
}

func (m *dmzManager) handleUDPForward(req *udp.ForwarderRequest) {
	id := req.ID()
	port := id.LocalPort
	targetHost, ok := m.target()
	if !ok {
		return
	}

	go func() {
		var wq waiter.Queue
		ep, err := req.CreateEndpoint(&wq)
		if err != nil {
			return
		}
		client := gonet.NewUDPConn(&wq, ep)

		addr, resolveErr := m.resolver.ResolveHost(targetHost)
		if resolveErr != nil {
			log.Printf("dmz udp resolve %q: %v", targetHost, resolveErr)
			_ = client.Close()
			return
		}
		target := netip.AddrPortFrom(addr, port)
		ctx := context.Background()
		backend, dialErr := m.tnet.DialContext(ctx, "udp", target.String())
		if dialErr != nil {
			log.Printf("dmz udp dial %s: %v", target, dialErr)
			_ = client.Close()
			return
		}

		log.Printf("dmz udp %s:%d -> %s", m.hubIP, port, target)

		sessionIdle := 2 * time.Minute
		done := make(chan struct{})
		go func() {
			defer close(done)
			buf := make([]byte, 64*1024)
			for {
				_ = backend.SetReadDeadline(time.Now().Add(sessionIdle))
				n, readErr := backend.Read(buf)
				if readErr != nil {
					return
				}
				if _, writeErr := client.Write(buf[:n]); writeErr != nil {
					return
				}
			}
		}()

		buf := make([]byte, 64*1024)
		for {
			_ = client.SetReadDeadline(time.Now().Add(sessionIdle))
			n, readErr := client.Read(buf)
			if readErr != nil {
				break
			}
			if _, writeErr := backend.Write(buf[:n]); writeErr != nil {
				break
			}
		}
		_ = backend.Close()
		_ = client.Close()
		<-done
	}()
}
