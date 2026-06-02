package integration

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"testing"
	"time"

	"github.com/miekg/dns"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/vpn/filter"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/wg"
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

func mustAtoi(s string) int {
	var n int
	fmt.Sscanf(s, "%d", &n)
	return n
}

func waitForHandshake(t *testing.T, mgr *wg.Manager, peerPubKey string, timeout time.Duration) error {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		stats, err := mgr.GetStats()
		if err != nil {
			return err
		}
		if s, ok := stats[peerPubKey]; ok && !s.LastHandshake.IsZero() {
			return nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("wireguard handshake timeout")
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

func applyAccessRules(hubMgr *wg.Manager, st *repo.Store) error {
	rules, err := buildAccessRulesFromStore(st)
	if err != nil {
		return err
	}
	hubMgr.SetAccessRules(rules)
	return nil
}

func toPortForwardRules(rules []repo.PortForward) []filter.PortForwardRule {
	out := make([]filter.PortForwardRule, 0, len(rules))
	for _, r := range rules {
		out = append(out, filter.PortForwardRule{
			ID:         r.ID,
			ListenPort: r.ListenPort,
			Protocol:   r.Protocol,
			TargetHost: r.TargetHost,
			TargetPort: r.TargetPort,
			Enabled:    r.Enabled,
		})
	}
	return out
}

func (env *peerMeshEnv) syncPortForwards(t *testing.T) {
	t.Helper()
	if env.portProxy == nil {
		mgr, err := filter.NewPortProxyManager(env.wgMgr.Net(), env.hubIP, env.dnsServer)
		if err != nil {
			t.Fatal(err)
		}
		env.portProxy = mgr
	}
	rules, err := env.store.ListPortForwards()
	if err != nil {
		t.Fatal(err)
	}
	if err := env.portProxy.Apply(toPortForwardRules(rules)); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)
}

func buildAccessRulesFromStore(st *repo.Store) (*filter.RuleSet, error) {
	peers, err := st.ListPeers()
	if err != nil {
		return nil, err
	}
	links, err := st.ListGroupLinks()
	if err != nil {
		return nil, err
	}
	eps := make([]domain.PeerEndpoint, len(peers))
	for i, p := range peers {
		eps[i] = domain.PeerEndpoint{
			ID: p.ID, WGIP: p.WGIP, GroupID: p.GroupID, Enabled: p.Enabled,
		}
	}
	pairs := make([]domain.GroupLinkPair, len(links))
	for i, l := range links {
		pairs[i] = domain.GroupLinkPair{FromGroupID: l.FromGroupID, ToGroupID: l.ToGroupID}
	}
	return domain.BuildAccessRules(eps, pairs)
}
