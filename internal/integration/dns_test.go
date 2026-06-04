package integration

import (
	"testing"

	"github.com/miekg/dns"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/peer"
)

func TestDNSHubAndPeer(t *testing.T) {
	env, hubNet, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()
	peerIP := env.peers[0].Peer.WGIP

	for _, qname := range []string{config.DNSDomain, "www." + config.DNSDomain} {
		if _, err := queryA(hubNet, env.dnsIP, qname); err == nil {
			t.Fatalf("apex %q should not resolve", qname)
		}
	}

	hubFQDN := peer.HubFQDN()
	if got := queryAOrFail(t, hubNet, env.dnsIP, hubFQDN); got != env.hubIP {
		t.Fatalf("hub %s = %s, want %s", hubFQDN, got, env.hubIP)
	}
	if got := queryAOrFail(t, hubNet, env.dnsIP, "www."+hubFQDN); got != env.hubIP {
		t.Fatalf("www hub = %s, want %s", got, env.hubIP)
	}
	if got := queryAOrFail(t, hubNet, env.dnsIP, "touken."+config.DNSDomain); got != peerIP {
		t.Fatalf("peer fqdn = %s, want %s", got, peerIP)
	}
	if got := queryAOrFail(t, hubNet, env.dnsIP, "www.touken."+config.DNSDomain); got != peerIP {
		t.Fatalf("www peer fqdn = %s, want %s", got, peerIP)
	}
}

func TestDNSAAAANODATA(t *testing.T) {
	env, hubNet, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()

	hubFQDN := peer.HubFQDN()
	rcode, err := queryRcode(hubNet, env.dnsIP, hubFQDN, dns.TypeAAAA)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeSuccess {
		t.Fatalf("AAAA %s rcode = %s, want NOERROR (NODATA for IPv4-only hub)", hubFQDN, dns.RcodeToString[rcode])
	}
}

func TestDNSHTTPSNODATA(t *testing.T) {
	env, hubNet, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()

	hubFQDN := peer.HubFQDN()
	rcode, err := queryRcode(hubNet, env.dnsIP, hubFQDN, dns.TypeHTTPS)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeSuccess {
		t.Fatalf("HTTPS %s rcode = %s, want NOERROR (NODATA for IPv4-only hub)", hubFQDN, dns.RcodeToString[rcode])
	}
}

func TestDNSExternalRefusedWithoutUpstream(t *testing.T) {
	env, hubNet, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()

	rcode, err := queryRcode(hubNet, env.dnsIP, "example.com", dns.TypeA)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeRefused {
		t.Fatalf("external name rcode = %s, want REFUSED without upstream", dns.RcodeToString[rcode])
	}
}

func TestDNSUnknownNXDOMAIN(t *testing.T) {
	env, hubNet, cleanup := setupMesh(t, []peerSpec{{Name: "touken"}}, nil)
	defer cleanup()

	rcode, err := queryRcode(hubNet, env.dnsIP, "nosuch."+config.DNSDomain, dns.TypeA)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeNameError {
		t.Fatalf("unknown name rcode = %s, want NXDOMAIN", dns.RcodeToString[rcode])
	}
}
