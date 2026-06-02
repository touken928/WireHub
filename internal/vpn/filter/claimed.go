package filter

// buildClaimedListenPorts returns hub VPN ports reserved or taken by enabled forwards.
// DMZ must not bind these; explicit forwards win over DMZ on the same port.
func buildClaimedListenPorts(hubWebPort int, rules []PortForwardRule) (tcpPorts, udpPorts map[uint16]struct{}) {
	tcpPorts = map[uint16]struct{}{53: {}}
	udpPorts = map[uint16]struct{}{53: {}}
	if hubWebPort >= 1 && hubWebPort <= 65535 {
		p := uint16(hubWebPort)
		tcpPorts[p] = struct{}{}
		udpPorts[p] = struct{}{}
	}
	for _, r := range rules {
		if !r.Enabled {
			continue
		}
		if r.ListenPort < 1 || r.ListenPort > 65535 {
			continue
		}
		p := uint16(r.ListenPort)
		switch r.Protocol {
		case "tcp":
			tcpPorts[p] = struct{}{}
		case "udp":
			udpPorts[p] = struct{}{}
		}
	}
	return tcpPorts, udpPorts
}
