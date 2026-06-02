package config

import (
	"fmt"
	"net"
)

// NthHostIP returns the n-th host address in a CIDR (1 = first host after network).
func NthHostIP(subnet string, n int) (string, error) {
	if n < 1 {
		return "", fmt.Errorf("host index must be >= 1")
	}
	ip, ipNet, err := net.ParseCIDR(subnet)
	if err != nil {
		return "", fmt.Errorf("parse subnet: %w", err)
	}
	ip = ip.To4()
	if ip == nil {
		return "", fmt.Errorf("only IPv4 subnets are supported")
	}

	host := make(net.IP, 4)
	copy(host, ip)
	for i := 0; i < n; i++ {
		for j := len(host) - 1; j >= 0; j-- {
			host[j]++
			if host[j] != 0 {
				break
			}
		}
	}
	if !ipNet.Contains(host) {
		return "", fmt.Errorf("subnet %s has no host at index %d", subnet, n)
	}
	return host.String(), nil
}

// FirstHostIP returns the first host address in a CIDR (network address + 1).
func FirstHostIP(subnet string) (string, error) {
	return NthHostIP(subnet, 1)
}
