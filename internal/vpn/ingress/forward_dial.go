package ingress

import (
	"context"
	"fmt"
	"net"
	"net/netip"
	"strings"
	"time"
)

const hostDialTimeout = 30 * time.Second

func parseVPNSubnet(subnet string) (*net.IPNet, error) {
	_, ipNet, err := net.ParseCIDR(subnet)
	if err != nil {
		return nil, fmt.Errorf("parse vpn subnet: %w", err)
	}
	return ipNet, nil
}

func addrInSubnet(subnet *net.IPNet, addr netip.Addr) bool {
	if subnet == nil {
		return false
	}
	ip := addr.AsSlice()
	if len(ip) == 0 {
		return false
	}
	return subnet.Contains(net.IP(ip))
}

func (m *ForwardProxy) dialTarget(ctx context.Context, network string, target netip.AddrPort) (net.Conn, error) {
	if addrInSubnet(m.vpnSubnet, target.Addr()) {
		return m.tnet.DialContext(ctx, network, target.String())
	}
	return dialHostNetwork(ctx, network, target.String(), m.vpnSubnet)
}

func dialHostNetwork(ctx context.Context, network, address string, vpnSubnet *net.IPNet) (net.Conn, error) {
	d := net.Dialer{Timeout: hostDialTimeout}
	if ip := chooseOutboundIPv4(vpnSubnet); ip != nil {
		switch {
		case strings.HasPrefix(network, "tcp"):
			d.LocalAddr = &net.TCPAddr{IP: ip}
		case strings.HasPrefix(network, "udp"):
			d.LocalAddr = &net.UDPAddr{IP: ip}
		}
	}
	return d.DialContext(ctx, network, address)
}

// chooseOutboundIPv4 picks a host IPv4 that routes outside the VPN subnet.
// When the hub machine is also a WireGuard peer, naive dials can loop through utun and time out.
func chooseOutboundIPv4(vpnSubnet *net.IPNet) net.IP {
	if ip := probeOutboundIPv4("114.114.114.114:53", vpnSubnet); ip != nil {
		return ip
	}
	return firstNonVPNIPv4(vpnSubnet)
}

func probeOutboundIPv4(remote string, vpnSubnet *net.IPNet) net.IP {
	conn, err := net.Dial("udp4", remote)
	if err != nil {
		return nil
	}
	defer conn.Close()
	udpAddr, ok := conn.LocalAddr().(*net.UDPAddr)
	if !ok || udpAddr.IP == nil {
		return nil
	}
	ip := udpAddr.IP.To4()
	if ip == nil || ip.IsLoopback() || addrInSubnet(vpnSubnet, netip.AddrFrom4([4]byte(ip))) {
		return nil
	}
	return ip
}

func firstNonVPNIPv4(vpnSubnet *net.IPNet) net.IP {
	ifaces, err := net.Interfaces()
	if err != nil {
		return nil
	}
	for _, iface := range ifaces {
		if iface.Flags&net.FlagUp == 0 || iface.Flags&net.FlagLoopback != 0 {
			continue
		}
		if strings.HasPrefix(iface.Name, "utun") {
			continue
		}
		addrs, err := iface.Addrs()
		if err != nil {
			continue
		}
		for _, a := range addrs {
			ipNet, ok := a.(*net.IPNet)
			if !ok || ipNet.IP.To4() == nil {
				continue
			}
			ip := ipNet.IP.To4()
			addr, ok := netip.AddrFromSlice(ip)
			if !ok || addrInSubnet(vpnSubnet, addr) {
				continue
			}
			return ip
		}
	}
	return nil
}
