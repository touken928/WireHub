package domain

import (
	"fmt"
	"net"
	"strings"

	"github.com/touken928/wirehub/internal/config"
)

const HubConfigVersion = 1

// HubConfig is the portable hub settings blob (no secrets).
type HubConfig struct {
	Version        int      `json:"version"`
	Endpoint       string   `json:"endpoint"`
	Subnet         string   `json:"subnet"`
	AdminUsername  string   `json:"admin_username"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

// ValidateHubConfig checks import/setup fields. When requireEndpoint is true, endpoint must be set.
func ValidateHubConfig(cfg HubConfig, requireEndpoint bool) error {
	if cfg.Version != 0 && cfg.Version != HubConfigVersion {
		return fmt.Errorf("unsupported config version %d", cfg.Version)
	}

	endpoint := strings.TrimSpace(cfg.Endpoint)
	if requireEndpoint && endpoint == "" {
		return fmt.Errorf("endpoint is required")
	}
	if endpoint != "" {
		if strings.Contains(endpoint, "://") {
			return fmt.Errorf("endpoint must be a hostname or IP, not a URL")
		}
		if host, _, err := net.SplitHostPort(endpoint); err == nil {
			endpoint = host
		}
		if ip := net.ParseIP(endpoint); ip == nil {
			if len(endpoint) > 253 {
				return fmt.Errorf("endpoint hostname too long")
			}
			for _, label := range strings.Split(endpoint, ".") {
				if label == "" || len(label) > 63 {
					return fmt.Errorf("invalid endpoint hostname")
				}
			}
		}
	}

	subnet := strings.TrimSpace(cfg.Subnet)
	if subnet == "" {
		subnet = config.DefaultSubnet
	}
	if _, err := config.FirstHostIP(subnet); err != nil {
		return fmt.Errorf("subnet: %w", err)
	}

	username := strings.TrimSpace(cfg.AdminUsername)
	if username == "" {
		username = config.DefaultAdminUsername
	}
	if len(username) < 2 || len(username) > 64 {
		return fmt.Errorf("admin username must be 2–64 characters")
	}

	mtu := cfg.MTU
	if mtu == 0 {
		mtu = config.DefaultMTU
	}
	if mtu < 1280 || mtu > 9000 {
		return fmt.Errorf("mtu must be between 1280 and 9000")
	}

	interval := cfg.StatusInterval
	if interval == 0 {
		interval = config.DefaultStatusInterval
	}
	if interval < 1 || interval > 3600 {
		return fmt.Errorf("status interval must be between 1 and 3600 seconds")
	}

	if _, err := config.ParseUpstreamDNS(cfg.UpstreamDNS); err != nil {
		return err
	}

	return nil
}

func NormalizeHubConfig(cfg HubConfig) HubConfig {
	out := cfg
	if out.Version == 0 {
		out.Version = HubConfigVersion
	}
	out.Endpoint = strings.TrimSpace(out.Endpoint)
	out.Subnet = strings.TrimSpace(out.Subnet)
	if out.Subnet == "" {
		out.Subnet = config.DefaultSubnet
	}
	out.AdminUsername = strings.TrimSpace(out.AdminUsername)
	if out.AdminUsername == "" {
		out.AdminUsername = config.DefaultAdminUsername
	}
	if out.MTU == 0 {
		out.MTU = config.DefaultMTU
	}
	if out.StatusInterval == 0 {
		out.StatusInterval = config.DefaultStatusInterval
	}
	dns, _ := config.ParseUpstreamDNS(out.UpstreamDNS)
	out.UpstreamDNS = dns
	return out
}
