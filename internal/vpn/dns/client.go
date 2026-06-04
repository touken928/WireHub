package dns

import (
	"net"

	"github.com/miekg/dns"
)

func dnsClientIP(w dns.ResponseWriter) string {
	host, _, err := net.SplitHostPort(w.RemoteAddr().String())
	if err != nil {
		return w.RemoteAddr().String()
	}
	return host
}
