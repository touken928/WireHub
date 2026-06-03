package domain

import (
	"fmt"
	"net"
	"strings"

	"github.com/touken928/wirehub/internal/vpn/filter/l4"
)

const (
	ForwardProtoTCP = "tcp"
	ForwardProtoUDP = "udp"
)

func normalizeForwardTargetHost(host string) string {
	return strings.TrimSuffix(strings.ToLower(strings.TrimSpace(host)), ".")
}

// ValidateForwardTargetHost checks the forward destination FQDN or IPv4 address.
// Peer names without a domain suffix are rejected.
func ValidateForwardTargetHost(host string) (string, error) {
	host = normalizeForwardTargetHost(host)
	if host == "" {
		return "", fmt.Errorf("target host is required")
	}
	if ip := net.ParseIP(host); ip != nil {
		if ip.To4() == nil {
			return "", fmt.Errorf("only IPv4 target addresses are supported")
		}
		return ip.String(), nil
	}
	if !strings.Contains(host, ".") {
		return "", fmt.Errorf("target host must be a hostname or IPv4 address, not a bare peer name")
	}
	if len(host) > 253 {
		return "", fmt.Errorf("target host too long")
	}
	for _, label := range strings.Split(host, ".") {
		if label == "" {
			return "", fmt.Errorf("invalid target host")
		}
		if len(label) > 63 {
			return "", fmt.Errorf("target host label too long")
		}
	}
	return host, nil
}

func ValidateForwardPort(port int, field string) error {
	if port < 1 || port > 65535 {
		return fmt.Errorf("%s must be between 1 and 65535", field)
	}
	return nil
}

func ValidateForwardProtocol(proto string) (string, error) {
	proto = strings.ToLower(strings.TrimSpace(proto))
	switch proto {
	case ForwardProtoTCP, ForwardProtoUDP:
		return proto, nil
	default:
		return "", fmt.Errorf("protocol must be tcp or udp")
	}
}

// ValidateForwardListenPort rejects ports reserved by hub services on the VPN address.
func ValidateForwardListenPort(port int, hubWebPort int, protocol string) error {
	if err := ValidateForwardPort(port, "listen port"); err != nil {
		return err
	}
	if port == l4.HubDNSPort {
		return fmt.Errorf("listen port %d is reserved for hub DNS", l4.HubDNSPort)
	}
	if port == hubWebPort {
		if protocol == ForwardProtoTCP {
			return fmt.Errorf("listen port %d is used by the hub web UI and API", hubWebPort)
		}
		return fmt.Errorf("listen port %d is used by WireGuard (UDP)", hubWebPort)
	}
	return nil
}

// ForwardDisplayHost returns a normalized hostname for UI.
func ForwardDisplayHost(host string) string {
	return normalizeForwardTargetHost(host)
}

// ForwardDisplayTarget returns host:port for UI.
func ForwardDisplayTarget(host string, port int) string {
	return fmt.Sprintf("%s:%d", ForwardDisplayHost(host), port)
}
