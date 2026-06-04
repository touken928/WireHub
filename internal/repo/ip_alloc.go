package repo

import (
	"errors"
	"fmt"
	"net"
)

var errSubnetIPUnavailable = errors.New("no available IP in subnet")

func (s *Store) collectUsedSubnetIPs(hubIP, dnsIP string) (map[string]bool, error) {
	used := map[string]bool{}
	if hubIP != "" {
		used[hubIP] = true
	}
	if dnsIP != "" {
		used[dnsIP] = true
	}

	var peers []Peer
	if err := s.db.Find(&peers).Error; err != nil {
		return nil, err
	}
	for _, p := range peers {
		used[p.WGIP] = true
	}

	var maps []ServiceMap
	if err := s.db.Find(&maps).Error; err != nil {
		return nil, err
	}
	for _, r := range maps {
		used[r.VirtualIP] = true
	}

	var records []DNSRecord
	if err := s.db.Find(&records).Error; err != nil {
		return nil, err
	}
	for _, rec := range records {
		used[rec.IP] = true
	}

	return used, nil
}

func nextFreeHostInSubnet(subnet string, used map[string]bool) (string, error) {
	_, ipNet, err := net.ParseCIDR(subnet)
	if err != nil {
		return "", err
	}
	base := ipNet.IP.To4()
	if base == nil {
		return "", fmt.Errorf("only IPv4 subnets supported")
	}
	mask, _ := ipNet.Mask.Size()
	for i := 2; i < (1 << (32 - mask)); i++ {
		ip := make(net.IP, 4)
		copy(ip, base)
		ip[3] = base[3] + byte(i)
		candidate := ip.String()
		if !used[candidate] {
			return candidate, nil
		}
	}
	return "", errSubnetIPUnavailable
}

func (s *Store) allocateSubnetIP(subnet, hubIP, dnsIP string) (string, error) {
	used, err := s.collectUsedSubnetIPs(hubIP, dnsIP)
	if err != nil {
		return "", err
	}
	return nextFreeHostInSubnet(subnet, used)
}
