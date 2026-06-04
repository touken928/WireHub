package domain

import (
	"fmt"
	"strings"

)

// ClientConfigInput holds hub and peer fields needed to render a WireGuard client config.
type ClientConfigInput struct {
	Endpoint         string
	ListenPort       int
	ServerPublicKey  string
	AllowedSubnet    string
	ClientDNS        []string
	PeerPrivateKey   string
	PeerAddress      string
}

// BuildClientConfig renders a WireGuard client configuration file.
func BuildClientConfig(in ClientConfigInput) (string, error) {
	if in.Endpoint == "" {
		return "", fmt.Errorf("server endpoint is not configured")
	}
	var b strings.Builder
	fmt.Fprintf(&b, "[Interface]\n")
	fmt.Fprintf(&b, "PrivateKey = %s\n", in.PeerPrivateKey)
	fmt.Fprintf(&b, "Address = %s/32\n", in.PeerAddress)
	fmt.Fprintf(&b, "DNS = %s\n", strings.Join(in.ClientDNS, ", "))
	fmt.Fprintf(&b, "# Hub web UI: http://%s/\n\n", HubFQDN())
	fmt.Fprintf(&b, "[Peer]\n")
	fmt.Fprintf(&b, "PublicKey = %s\n", in.ServerPublicKey)
	fmt.Fprintf(&b, "Endpoint = %s:%d\n", in.Endpoint, in.ListenPort)
	fmt.Fprintf(&b, "PersistentKeepalive = 25\n")
	fmt.Fprintf(&b, "AllowedIPs = %s\n", in.AllowedSubnet)
	return b.String(), nil
}
