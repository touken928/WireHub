package l4

import "sort"

// ForwardRule is an admin-configured forward (hub VPN IP:listenPort → target).
type ForwardRule struct {
	ID         uint
	ListenPort int
	Protocol   string
	TargetHost string
	TargetPort int
	Enabled    bool
}

func forwardListenPorts(rules []ForwardRule) []int {
	out := make([]int, 0, len(rules))
	for _, r := range rules {
		if r.Enabled {
			out = append(out, r.ListenPort)
		}
	}
	return out
}

// ReservedHubPorts lists hub port numbers that must not be used as SNAT ephemeral ports.
// Includes system listeners (DNS, Web/WG CLI port) and enabled Forward listen ports.
func ReservedHubPorts(webTCPPort int, forwards []ForwardRule) []int {
	seen := make(map[int]struct{})
	add := func(p int) {
		if p >= 1 && p <= 65535 {
			seen[p] = struct{}{}
		}
	}
	add(HubDNSPort)
	add(webTCPPort)
	for _, p := range forwardListenPorts(forwards) {
		add(p)
	}
	out := make([]int, 0, len(seen))
	for p := range seen {
		out = append(out, p)
	}
	sort.Ints(out)
	return out
}
