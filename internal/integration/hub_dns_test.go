package integration

import (
	"testing"

	"github.com/touken928/wirehub/internal/config"
)

func TestHubDNSApexAndPeer(t *testing.T) {
	env, tnet, cleanup := setupHub(t)
	defer cleanup()

	if got := queryAOrFail(t, tnet, env.dnsIP, config.DNSDomain); got != env.hubIP {
		t.Fatalf("apex %s = %s, want %s", config.DNSDomain, got, env.hubIP)
	}
	if got := queryAOrFail(t, tnet, env.dnsIP, "www."+config.DNSDomain); got != env.hubIP {
		t.Fatalf("www = %s, want %s", got, env.hubIP)
	}
	if got := queryAOrFail(t, tnet, env.dnsIP, "touken."+config.DNSDomain); got != env.peerIP {
		t.Fatalf("peer fqdn = %s, want %s", got, env.peerIP)
	}
	if got := queryAOrFail(t, tnet, env.dnsIP, "www.touken."+config.DNSDomain); got != env.peerIP {
		t.Fatalf("www peer fqdn = %s, want %s", got, env.peerIP)
	}
}
