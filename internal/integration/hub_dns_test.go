package integration

import (
	"testing"

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
