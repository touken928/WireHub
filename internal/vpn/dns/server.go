package dns

import (
	"fmt"
	"log"
	"net"
	"net/netip"
	"strings"
	"sync"
	"time"

	"github.com/miekg/dns"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

type Server struct {
	store    *repo.Store
	hubIP    string
	dnsIP    string
	upstream []string
	server   *dns.Server
	serveMu  sync.Mutex
	stopCh   chan struct{}
	stopOnce sync.Once
}

// SetUpstream replaces upstream resolver list used for external queries.
func (s *Server) SetUpstream(upstream []string) {
	s.upstream = append([]string(nil), upstream...)
}

func NewServer(st *repo.Store, hubIP, dnsIP string, upstream []string) *Server {
	up := append([]string(nil), upstream...)
	return &Server{
		store:    st,
		hubIP:    hubIP,
		dnsIP:    dnsIP,
		upstream: up,
	}
}

func (s *Server) StartOnNetstack(tnet *netstack.Net, dnsIP string, port int) error {
	if s.stopCh == nil {
		s.stopCh = make(chan struct{})
	}
	conn, err := s.listenUDP(tnet, dnsIP, port)
	if err != nil {
		return err
	}
	go s.serveNetstack(tnet, dnsIP, port, conn)
	log.Printf("dns listening on %s:%d (netstack, domain %s, upstream %v)", dnsIP, port, config.DNSDomain, s.upstream)
	return nil
}

func (s *Server) listenUDP(tnet *netstack.Net, dnsIP string, port int) (net.PacketConn, error) {
	addr, err := netip.ParseAddr(dnsIP)
	if err != nil {
		return nil, fmt.Errorf("parse dns ip: %w", err)
	}
	conn, err := tnet.ListenUDPAddrPort(netip.AddrPortFrom(addr, uint16(port)))
	if err != nil {
		return nil, fmt.Errorf("listen udp on netstack %s:%d: %w", dnsIP, port, err)
	}
	return conn, nil
}

func (s *Server) serveNetstack(tnet *netstack.Net, dnsIP string, port int, conn net.PacketConn) {
	for {
		srv := &dns.Server{
			PacketConn: conn,
			Handler:    dns.HandlerFunc(s.handle),
		}
		s.serveMu.Lock()
		s.server = srv
		s.serveMu.Unlock()
		if err := srv.ActivateAndServe(); err != nil {
			log.Printf("dns server stopped: %v", err)
		}
		_ = conn.Close()
		if !s.waitOrStop(time.Second) {
			return
		}
		var err error
		conn, err = s.listenUDP(tnet, dnsIP, port)
		if err != nil {
			log.Printf("dns listen on netstack %s:%d: %v", dnsIP, port, err)
			if !s.waitOrStop(2 * time.Second) {
				return
			}
			continue
		}
	}
}

func (s *Server) waitOrStop(d time.Duration) bool {
	if d == 0 {
		select {
		case <-s.stopCh:
			return false
		default:
			return true
		}
	}
	select {
	case <-s.stopCh:
		return false
	case <-time.After(d):
		return true
	}
}

func (s *Server) Stop() error {
	var err error
	s.stopOnce.Do(func() {
		close(s.stopCh)
	})
	s.serveMu.Lock()
	srv := s.server
	s.serveMu.Unlock()
	if srv != nil {
		err = srv.Shutdown()
	}
	return err
}

func (s *Server) isInternalName(name string) bool {
	domain := config.DNSDomain
	name = strings.TrimSuffix(strings.ToLower(name), ".")
	if name == domain {
		return true
	}
	return strings.HasSuffix(name, "."+domain)
}

func (s *Server) handle(w dns.ResponseWriter, r *dns.Msg) {
	m := new(dns.Msg)
	m.SetReply(r)
	m.Authoritative = true

	var external []dns.Question
	internalFound := false
	internalMissing := false
	for _, q := range r.Question {
		name := strings.TrimSuffix(strings.ToLower(q.Name), ".")
		if !s.isInternalName(name) {
			external = append(external, q)
			continue
		}

		var slug string
		var ok bool
		domain := config.DNSDomain
		if strings.HasSuffix(name, "."+domain) {
			slug = strings.TrimSuffix(name, "."+domain)
			if slug == "" {
				internalMissing = true
				continue
			}
			ok = true
		} else {
			internalMissing = true
			continue
		}

		ip, resolved := s.lookupIPForClient(slug, ok, dnsClientIP(w))
		if !resolved {
			internalMissing = true
			continue
		}
		internalFound = true

		switch q.Qtype {
		case dns.TypeA:
			if parsed := net.ParseIP(ip); parsed != nil && parsed.To4() != nil {
				m.Answer = append(m.Answer, &dns.A{
					Hdr: dns.RR_Header{Name: q.Name, Rrtype: dns.TypeA, Class: dns.ClassINET, Ttl: 60},
					A:   parsed.To4(),
				})
			}
		case dns.TypeAAAA:
			if parsed := net.ParseIP(ip); parsed != nil && parsed.To4() == nil {
				m.Answer = append(m.Answer, &dns.AAAA{
					Hdr:  dns.RR_Header{Name: q.Name, Rrtype: dns.TypeAAAA, Class: dns.ClassINET, Ttl: 60},
					AAAA: parsed,
				})
			}
		}
	}

	if len(external) > 0 {
		if len(s.upstream) == 0 {
			if len(m.Answer) == 0 {
				m.Rcode = dns.RcodeRefused
			}
		} else {
			fwd := r.Copy()
			fwd.Question = external
			if resp, err := s.exchangeUpstream(fwd); err == nil && resp != nil {
				m.Answer = append(m.Answer, resp.Answer...)
				m.Ns = append(m.Ns, resp.Ns...)
				m.Extra = append(m.Extra, resp.Extra...)
				if len(resp.Answer) > 0 {
					m.Rcode = resp.Rcode
					m.Authoritative = resp.Authoritative
				}
			} else if len(m.Answer) == 0 {
				m.Rcode = dns.RcodeServerFailure
			}
		}
	}

	if len(m.Answer) == 0 && m.Rcode == dns.RcodeSuccess {
		// NXDOMAIN only when the name is unknown. IPv4-only names must answer
		// AAAA with NOERROR NODATA so getaddrinfo/curl on macOS can fall back to A.
		if internalMissing && !internalFound {
			m.Rcode = dns.RcodeNameError
		}
	}
	_ = w.WriteMsg(m)
}

func (s *Server) exchangeUpstream(req *dns.Msg) (*dns.Msg, error) {
	return exchangeDNS(req, s.upstream)
}

func (s *Server) RegisterPeer(peer *repo.Peer) error {
	slug := peer.Name
	if slug == "" {
		slug = domain.HostnameSlug(peer.Name)
	}
	peer.DNSName = slug
	_ = s.store.DeleteDNSByPeerID(peer.ID)
	record := &repo.DNSRecord{
		Hostname: slug,
		IP:       peer.WGIP,
		PeerID:   &peer.ID,
		Manual:   false,
	}
	return s.store.CreateDNSRecord(record)
}
