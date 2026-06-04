package integration

import (
	"fmt"
	"net"
	"testing"

	"github.com/miekg/dns"
	"github.com/touken928/wirehub/internal/repo"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

func ensurePeerDNSRecord(st *repo.Store, peer *repo.Peer) error {
	slug := peer.DNSName
	if slug == "" {
		slug = peer.Name
	}
	peer.DNSName = slug
	_ = st.DeleteDNSByPeerID(peer.ID)
	return st.CreateDNSRecord(&repo.DNSRecord{
		Hostname: slug,
		IP:       peer.WGIP,
		PeerID:   &peer.ID,
		Manual:   false,
	})
}

func queryA(tnet *netstack.Net, dnsIP, qname string) (string, error) {
	msg := new(dns.Msg)
	msg.SetQuestion(dns.Fqdn(qname), dns.TypeA)
	pack, err := msg.Pack()
	if err != nil {
		return "", err
	}
	raddr := &net.UDPAddr{IP: net.ParseIP(dnsIP), Port: 53}
	conn, err := tnet.DialUDP(nil, raddr)
	if err != nil {
		return "", err
	}
	defer conn.Close()
	if _, err := conn.Write(pack); err != nil {
		return "", err
	}
	buf := make([]byte, 512)
	n, err := conn.Read(buf)
	if err != nil {
		return "", err
	}
	resp := new(dns.Msg)
	if err := resp.Unpack(buf[:n]); err != nil {
		return "", err
	}
	if resp.Rcode != dns.RcodeSuccess || len(resp.Answer) == 0 {
		return "", fmt.Errorf("no answer for %s (rcode=%s)", qname, dns.RcodeToString[resp.Rcode])
	}
	if a, ok := resp.Answer[0].(*dns.A); ok {
		return a.A.String(), nil
	}
	return "", fmt.Errorf("unexpected rr type")
}

func queryAOrFail(t *testing.T, tnet *netstack.Net, dnsIP, qname string) string {
	t.Helper()
	ip, err := queryA(tnet, dnsIP, qname)
	if err != nil {
		t.Fatal(err)
	}
	return ip
}

func queryRcode(tnet *netstack.Net, dnsIP, qname string, qtype uint16) (int, error) {
	msg := new(dns.Msg)
	msg.SetQuestion(dns.Fqdn(qname), qtype)
	pack, err := msg.Pack()
	if err != nil {
		return 0, err
	}
	raddr := &net.UDPAddr{IP: net.ParseIP(dnsIP), Port: 53}
	conn, err := tnet.DialUDP(nil, raddr)
	if err != nil {
		return 0, err
	}
	defer conn.Close()
	if _, err := conn.Write(pack); err != nil {
		return 0, err
	}
	buf := make([]byte, 512)
	n, err := conn.Read(buf)
	if err != nil {
		return 0, err
	}
	resp := new(dns.Msg)
	if err := resp.Unpack(buf[:n]); err != nil {
		return 0, err
	}
	return resp.Rcode, nil
}
