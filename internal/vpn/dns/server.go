package dns

import (
	"fmt"
	"log"
	"net"
	"net/netip"
	"strings"
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
	addr, err := netip.ParseAddr(dnsIP)
	if err != nil {
		return fmt.Errorf("parse dns ip: %w", err)
	}

	conn, err := tnet.ListenUDPAddrPort(netip.AddrPortFrom(addr, uint16(port)))
	if err != nil {
		return fmt.Errorf("listen udp on netstack %s:%d: %w", dnsIP, port, err)
	}

	s.server = &dns.Server{
		PacketConn: conn,
		Handler:    dns.HandlerFunc(s.handle),
	}
	go func() {
		if err := s.server.ActivateAndServe(); err != nil {
			log.Printf("dns server stopped: %v", err)
		}
	}()
	log.Printf("dns listening on %s:%d (netstack, domain %s, upstream %v)", dnsIP, port, config.DNSDomain, s.upstream)
	return nil
}

func (s *Server) Stop() error {
	if s.server != nil {
		return s.server.Shutdown()
	}
	return nil
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
				continue
			}
			ok = true
		} else {
			continue
		}

		ip, resolved := s.lookupIP(slug, ok)
		if !resolved {
			continue
		}

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

	if len(external) > 0 && len(s.upstream) > 0 {
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

	if len(m.Answer) == 0 && m.Rcode == dns.RcodeSuccess {
		m.Rcode = dns.RcodeNameError
	}
	_ = w.WriteMsg(m)
}

func (s *Server) exchangeUpstream(req *dns.Msg) (*dns.Msg, error) {
	client := &dns.Client{Net: "udp", Timeout: 3 * time.Second}
	for _, upstream := range s.upstream {
		target := net.JoinHostPort(upstream, "53")
		resp, _, err := client.Exchange(req, target)
		if err != nil {
			continue
		}
		if resp != nil {
			return resp, nil
		}
	}
	return nil, fmt.Errorf("upstream dns unavailable")
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
