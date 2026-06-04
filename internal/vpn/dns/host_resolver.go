package dns

import (
	"fmt"
	"net"
	"time"

	"github.com/miekg/dns"
)

// exchangeDNS queries resolvers over the host network (UDP/TCP 53), bypassing OS/VPN DNS.
func exchangeDNS(req *dns.Msg, resolvers []string) (*dns.Msg, error) {
	if len(resolvers) == 0 {
		return nil, fmt.Errorf("no upstream DNS configured")
	}
	client := &dns.Client{Timeout: 1500 * time.Millisecond}
	for _, netw := range []string{"udp", "tcp"} {
		for _, upstream := range resolvers {
			client.Net = netw
			target := net.JoinHostPort(upstream, "53")
			resp, _, err := client.Exchange(req, target)
			if err == nil && resp != nil && resp.Rcode == dns.RcodeSuccess && len(resp.Answer) > 0 {
				return resp, nil
			}
		}
	}
	return nil, fmt.Errorf("upstream dns unavailable")
}
