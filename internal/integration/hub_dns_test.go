package integration

import (
	"testing"

	"github.com/miekg/dns"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain"
)

func TestHubDNSHubAndPeer(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	for _, qname := range []string{config.DNSDomain, "www." + config.DNSDomain} {
		if _, err := queryA(tnet, env.dnsIP, qname); err == nil {
			t.Fatalf("apex %q should not resolve", qname)
		}
	}

	hubFQDN := domain.HubFQDN()
	if got := queryAOrFail(t, tnet, env.dnsIP, hubFQDN); got != env.hubIP {
		t.Fatalf("hub %s = %s, want %s", hubFQDN, got, env.hubIP)
	}
	if got := queryAOrFail(t, tnet, env.dnsIP, "www."+hubFQDN); got != env.hubIP {
		t.Fatalf("www hub = %s, want %s", got, env.hubIP)
	}
	if got := queryAOrFail(t, tnet, env.dnsIP, "touken."+config.DNSDomain); got != env.peerIP {
		t.Fatalf("peer fqdn = %s, want %s", got, env.peerIP)
	}
	if got := queryAOrFail(t, tnet, env.dnsIP, "www.touken."+config.DNSDomain); got != env.peerIP {
		t.Fatalf("www peer fqdn = %s, want %s", got, env.peerIP)
	}
}

func TestHubDNSAAAANODATA(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	hubFQDN := domain.HubFQDN()
	rcode, err := queryRcode(tnet, env.dnsIP, hubFQDN, dns.TypeAAAA)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeSuccess {
		t.Fatalf("AAAA %s rcode = %s, want NOERROR (NODATA for IPv4-only hub)", hubFQDN, dns.RcodeToString[rcode])
	}
}

func TestHubDNSHTTPSNODATA(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	hubFQDN := domain.HubFQDN()
	rcode, err := queryRcode(tnet, env.dnsIP, hubFQDN, dns.TypeHTTPS)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeSuccess {
		t.Fatalf("HTTPS %s rcode = %s, want NOERROR (NODATA for IPv4-only hub)", hubFQDN, dns.RcodeToString[rcode])
	}
}

func TestHubDNSExternalRefusedWithoutUpstream(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	rcode, err := queryRcode(tnet, env.dnsIP, "example.com", dns.TypeA)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeRefused {
		t.Fatalf("external name rcode = %s, want REFUSED without upstream", dns.RcodeToString[rcode])
	}
}

func TestHubDNSUnknownNXDOMAIN(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	rcode, err := queryRcode(tnet, env.dnsIP, "nosuch."+config.DNSDomain, dns.TypeA)
	if err != nil {
		t.Fatal(err)
	}
	if rcode != dns.RcodeNameError {
		t.Fatalf("unknown name rcode = %s, want NXDOMAIN", dns.RcodeToString[rcode])
	}
}
