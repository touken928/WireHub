package l4

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
}

// ForwardProxy listens on hub VPN IP ports and relays to configured targets (admin Forward rules).
type ForwardProxy struct {
	tnet     *netstack.Net
	hubIP    netip.Addr
	resolver HostResolver

	mu     sync.Mutex
	cancel context.CancelFunc
	wg     sync.WaitGroup
}

func NewForwardProxy(tnet *netstack.Net, hubIP string, resolver HostResolver) (*ForwardProxy, error) {
	addr, err := netip.ParseAddr(hubIP)
	if err != nil {
		return nil, fmt.Errorf("parse hub ip: %w", err)
	}
	return &ForwardProxy{
		tnet:     tnet,
		hubIP:    addr,
		resolver: resolver,
	}, nil
}

func (m *ForwardProxy) Apply(rules []ForwardRule) error {
	m.Stop()
	if len(rules) == 0 {
		return nil
	}

	ctx, cancel := context.WithCancel(context.Background())
	m.mu.Lock()
	m.cancel = cancel
	m.mu.Unlock()

	for _, rule := range rules {
		if !rule.Enabled {
			continue
		}
		rule := rule
		m.wg.Add(1)
		go func() {
			defer m.wg.Done()
			if err := m.runRule(ctx, rule); err != nil && !errors.Is(err, context.Canceled) {
				log.Printf("forward %s/%d -> %s:%d: %v",
					rule.Protocol, rule.ListenPort, rule.TargetHost, rule.TargetPort, err)
			}
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

func (m *ForwardProxy) runRule(ctx context.Context, rule ForwardRule) error {
	switch rule.Protocol {
	case "tcp":
		return m.runTCP(ctx, rule)
	case "udp":
		return m.runUDP(ctx, rule)
	default:
		return fmt.Errorf("unsupported protocol %q", rule.Protocol)
	}
}

func (m *ForwardProxy) runTCP(ctx context.Context, rule ForwardRule) error {
	listen := netip.AddrPortFrom(m.hubIP, uint16(rule.ListenPort))
	ln, err := m.tnet.ListenTCPAddrPort(listen)
	if err != nil {
		return fmt.Errorf("listen tcp %s: %w", listen, err)
	}
	defer ln.Close()
	log.Printf("forward tcp %s -> %s:%d", listen, rule.TargetHost, rule.TargetPort)

	go func() {
		<-ctx.Done()
		_ = ln.Close()
	}()

	for {
		client, err := ln.Accept()
		if err != nil {
			if ctx.Err() != nil || errors.Is(err, net.ErrClosed) {
				return ctx.Err()
			}
			return err
		}
		go m.proxyTCP(ctx, client, rule.TargetHost, rule.TargetPort)
	}
}

func (m *ForwardProxy) proxyTCP(ctx context.Context, client net.Conn, targetHost string, targetPort int) {
	defer client.Close()

	addr, err := m.resolver.ResolveHost(targetHost)
	if err != nil {
		log.Printf("forward tcp resolve %q: %v", targetHost, err)
		return
	}
	target := netip.AddrPortFrom(addr, uint16(targetPort))
	remote, err := m.tnet.DialContext(ctx, "tcp", target.String())
	if err != nil {
		log.Printf("forward tcp dial %s: %v", target, err)
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

func (m *ForwardProxy) runUDP(ctx context.Context, rule ForwardRule) error {
	listen := netip.AddrPortFrom(m.hubIP, uint16(rule.ListenPort))
	pc, err := m.tnet.ListenUDPAddrPort(listen)
	if err != nil {
		return fmt.Errorf("listen udp %s: %w", listen, err)
	}
	defer pc.Close()
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
			return ctx.Err()
		}
		_ = pc.SetReadDeadline(time.Now().Add(readWait))
		n, clientAddr, err := pc.ReadFrom(buf)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
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
			return err
		}

		key := clientAddr.String()
		mu.Lock()
		sess, ok := sessions[key]
		if !ok {
			addr, resolveErr := m.resolver.ResolveHost(rule.TargetHost)
			if resolveErr != nil {
				mu.Unlock()
				log.Printf("forward udp resolve %q: %v", rule.TargetHost, resolveErr)
				continue
			}
			target := netip.AddrPortFrom(addr, uint16(rule.TargetPort))
			backend, dialErr := m.tnet.DialContext(ctx, "udp", target.String())
			if dialErr != nil {
				mu.Unlock()
				log.Printf("forward udp dial %s: %v", target, dialErr)
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
