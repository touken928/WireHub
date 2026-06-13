package ingress

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"net/netip"
	"sync"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

// HostResolver resolves forward target hostnames to IPv4 addresses.
type HostResolver interface {
	ResolveHost(host string) (netip.Addr, error)
	ResolveForwardAddrs(host string) ([]netip.Addr, error)
}

// ForwardProxy listens on hub VPN IP ports and maps to configured targets (admin Forward rules).
type ForwardProxy struct {
	tnet      *netstack.Net
	hubIP     netip.Addr
	vpnSubnet *net.IPNet
	resolver  HostResolver

	mu     sync.Mutex
	cancel context.CancelFunc
	wg     sync.WaitGroup
}

// boundListener holds a pre-bound listener for a forward rule.
type boundListener struct {
	rule  ForwardRule
	tcpLn net.Listener     // set for TCP rules
	udpPc net.PacketConn   // set for UDP rules
}

func (bl *boundListener) close() {
	if bl.tcpLn != nil {
		bl.tcpLn.Close()
	}
	if bl.udpPc != nil {
		bl.udpPc.Close()
	}
}

func NewForwardProxy(tnet *netstack.Net, hubIP, vpnSubnet string, resolver HostResolver) (*ForwardProxy, error) {
	addr, err := netip.ParseAddr(hubIP)
	if err != nil {
		return nil, fmt.Errorf("parse hub ip: %w", err)
	}
	subnet, err := parseVPNSubnet(vpnSubnet)
	if err != nil {
		return nil, err
	}
	return &ForwardProxy{
		tnet:      tnet,
		hubIP:     addr,
		vpnSubnet: subnet,
		resolver:  resolver,
	}, nil
}

// validateForwardRules checks all rules for basic validity.
func validateForwardRules(rules []ForwardRule) error {
	seen := make(map[int]int) // port → first rule index
	for i, rule := range rules {
		switch rule.Protocol {
		case "tcp", "udp":
		default:
			return fmt.Errorf("rule %d: unsupported protocol %q", i, rule.Protocol)
		}
		if rule.ListenPort <= 0 || rule.ListenPort > 65535 {
			return fmt.Errorf("rule %d: invalid listen port %d", i, rule.ListenPort)
		}
		if prev, ok := seen[rule.ListenPort]; ok {
			return fmt.Errorf("rules %d and %d: duplicate listen port %d", prev, i, rule.ListenPort)
		}
		seen[rule.ListenPort] = i
	}
	return nil
}

// Apply replaces all forward listeners with the given rules.
// It validates rules synchronously, eagerly binds all listeners,
// and returns an error if any rule is invalid or any bind fails.
// On failure, any successfully pre-bound listeners are closed (rollback).
func (m *ForwardProxy) Apply(rules []ForwardRule) error {
	// 1. Validate all rules synchronously.
	if err := validateForwardRules(rules); err != nil {
		return err
	}

	// 2. Stop old listeners (frees ports for re-bind).
	m.Stop()
	if len(rules) == 0 {
		return nil
	}

	// 3. Eagerly bind all new listeners synchronously.
	var listeners []boundListener
	for _, rule := range rules {
		bl, err := m.bindRule(rule)
		if err != nil {
			// Rollback: close all successfully bound listeners.
			for _, l := range listeners {
				l.close()
			}
			return fmt.Errorf("bind %s/%d: %w", rule.Protocol, rule.ListenPort, err)
		}
		listeners = append(listeners, bl)
	}

	// 4. All binds succeeded — start serving goroutines.
	ctx, cancel := context.WithCancel(context.Background())
	m.mu.Lock()
	m.cancel = cancel
	m.mu.Unlock()

	for _, bl := range listeners {
		bl := bl
		m.wg.Add(1)
		go func() {
			defer m.wg.Done()
			m.serveBound(ctx, bl)
		}()
	}
	return nil
}

func (m *ForwardProxy) Stop() {
	m.mu.Lock()
	cancel := m.cancel
	m.cancel = nil
	m.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	m.wg.Wait()
}

// bindRule synchronously binds a single forward rule and returns the listener.
func (m *ForwardProxy) bindRule(rule ForwardRule) (boundListener, error) {
	switch rule.Protocol {
	case "tcp":
		listen := netip.AddrPortFrom(m.hubIP, uint16(rule.ListenPort))
		ln, err := m.tnet.ListenTCPAddrPort(listen)
		if err != nil {
			return boundListener{}, err
		}
		return boundListener{rule: rule, tcpLn: ln}, nil
	case "udp":
		listen := netip.AddrPortFrom(m.hubIP, uint16(rule.ListenPort))
		pc, err := m.tnet.ListenUDPAddrPort(listen)
		if err != nil {
			return boundListener{}, err
		}
		return boundListener{rule: rule, udpPc: pc}, nil
	default:
		return boundListener{}, fmt.Errorf("unsupported protocol %q", rule.Protocol)
	}
}

// serveBound runs the accept loop for a pre-bound listener (long-lived goroutine).
func (m *ForwardProxy) serveBound(ctx context.Context, bl boundListener) {
	switch bl.rule.Protocol {
	case "tcp":
		m.serveTCP(ctx, bl.tcpLn, bl.rule)
	case "udp":
		m.serveUDP(ctx, bl.udpPc, bl.rule)
	}
}

func (m *ForwardProxy) serveTCP(ctx context.Context, ln net.Listener, rule ForwardRule) {
	defer ln.Close()
	listen := netip.AddrPortFrom(m.hubIP, uint16(rule.ListenPort))
	log.Printf("forward tcp %s -> %s:%d", listen, rule.TargetHost, rule.TargetPort)

	go func() {
		<-ctx.Done()
		_ = ln.Close()
	}()

	for {
		client, err := ln.Accept()
		if err != nil {
			if ctx.Err() != nil || errors.Is(err, net.ErrClosed) {
				return
			}
			log.Printf("forward tcp %s accept: %v", listen, err)
			return
		}
		go m.proxyTCP(ctx, client, rule.TargetHost, rule.TargetPort)
	}
}

func (m *ForwardProxy) proxyTCP(ctx context.Context, client net.Conn, targetHost string, targetPort int) {
	defer client.Close()

	addrs, err := m.resolver.ResolveForwardAddrs(targetHost)
	if err != nil {
		log.Printf("forward tcp resolve %q: %v", targetHost, err)
		return
	}
	var remote net.Conn
	var target netip.AddrPort
	for _, addr := range addrs {
		target = netip.AddrPortFrom(addr, uint16(targetPort))
		remote, err = m.dialTarget(ctx, "tcp", target)
		if err == nil {
			break
		}
		log.Printf("forward tcp dial %s: %v", target, err)
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

func (m *ForwardProxy) serveUDP(ctx context.Context, pc net.PacketConn, rule ForwardRule) {
	defer pc.Close()
	listen := netip.AddrPortFrom(m.hubIP, uint16(rule.ListenPort))
	log.Printf("forward udp %s -> %s:%d", listen, rule.TargetHost, rule.TargetPort)

	type session struct {
		backend    net.Conn
		lastActive time.Time
	}
	sessions := make(map[string]*session)
	var mu sync.Mutex

	buf := make([]byte, 64*1024)
	const readWait = 500 * time.Millisecond

	for {
		if ctx.Err() != nil {
			mu.Lock()
			for _, s := range sessions {
				_ = s.backend.Close()
			}
			mu.Unlock()
			return
		}
		_ = pc.SetReadDeadline(time.Now().Add(readWait))
		n, clientAddr, err := pc.ReadFrom(buf)
		if err != nil {
			if ctx.Err() != nil {
				return
			}
			if ne, ok := err.(net.Error); ok && ne.Timeout() {
				mu.Lock()
				now := time.Now()
				for key, s := range sessions {
					if s.lastActive.Add(SessionIdle).Before(now) {
						_ = s.backend.Close()
						delete(sessions, key)
					}
				}
				mu.Unlock()
				continue
			}
			log.Printf("forward udp %s accept: %v", listen, err)
			return
		}

		key := clientAddr.String()
		mu.Lock()
		sess, ok := sessions[key]
		if !ok {
			addrs, resolveErr := m.resolver.ResolveForwardAddrs(rule.TargetHost)
			if resolveErr != nil {
				mu.Unlock()
				log.Printf("forward udp resolve %q: %v", rule.TargetHost, resolveErr)
				continue
			}
			var backend net.Conn
			var dialErr error
			for _, addr := range addrs {
				target := netip.AddrPortFrom(addr, uint16(rule.TargetPort))
				backend, dialErr = m.dialTarget(ctx, "udp", target)
				if dialErr == nil {
					break
				}
				log.Printf("forward udp dial %s: %v", target, dialErr)
			}
			if dialErr != nil {
				mu.Unlock()
				continue
			}
			sess = &session{backend: backend, lastActive: time.Now()}
			sessions[key] = sess
			go func(client net.Addr, back net.Conn) {
				defer back.Close()
				b := make([]byte, 64*1024)
				for {
					if ctx.Err() != nil {
						return
					}
					_ = back.SetReadDeadline(time.Now().Add(SessionIdle))
					rn, readErr := back.Read(b)
					if readErr != nil {
						return
					}
					if _, writeErr := pc.WriteTo(b[:rn], client); writeErr != nil {
						return
					}
				}
			}(clientAddr, backend)
		}
		sess.lastActive = time.Now()
		mu.Unlock()
		if _, err := sess.backend.Write(buf[:n]); err != nil {
			mu.Lock()
			_ = sess.backend.Close()
			delete(sessions, key)
			mu.Unlock()
		}
	}
}
