package domain

import (
	"fmt"
	"net"
	"strings"

	"github.com/touken928/wirehub/internal/config"
)

const (
	ForwardProtoTCP = "tcp"
	ForwardProtoUDP = "udp"
)

// NormalizeForwardTargetHost accepts a peer label, wirehub FQDN, or external hostname.
func NormalizeForwardTargetHost(host string) string {
	host = strings.TrimSuffix(strings.ToLower(strings.TrimSpace(host)), ".")
	if host == "" {
		return ""
	}
	if strings.Contains(host, ".") {
		return host
	}
	return PeerFQDN(host)
}

// ValidateForwardTargetHost checks the forward destination hostname or IPv4 address.
func ValidateForwardTargetHost(host string) (string, error) {
	host = NormalizeForwardTargetHost(host)
	if host == "" {
		return "", fmt.Errorf("target host is required")
	}
	if ip := net.ParseIP(host); ip != nil {
		if ip.To4() == nil {
			return "", fmt.Errorf("only IPv4 target addresses are supported")
		}
		return ip.String(), nil
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
	if port == 53 {
		return fmt.Errorf("listen port 53 is reserved for hub DNS")
	}
	if port == hubWebPort {
		if protocol == ForwardProtoTCP {
			return fmt.Errorf("listen port %d is used by the hub web UI and API", hubWebPort)
		}
		return fmt.Errorf("listen port %d is used by WireGuard (UDP)", hubWebPort)
	}
	return nil
}

// ForwardDisplayTarget returns host:port for UI, shortening internal wirehub names when helpful.
func ForwardDisplayTarget(host string, port int) string {
	host = strings.TrimSuffix(strings.ToLower(host), ".")
	suffix := "." + config.DNSDomain
	if strings.HasSuffix(host, suffix) {
		label := strings.TrimSuffix(host, suffix)
		if label != "" && !strings.Contains(label, ".") {
			return fmt.Sprintf("%s:%d", label, port)
		}
	}
	return fmt.Sprintf("%s:%d", host, port)
}
