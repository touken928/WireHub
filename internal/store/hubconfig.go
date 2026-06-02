package store

import (
	"fmt"
	"net"
	"strings"

	"github.com/touken928/wirehub/internal/config"
	"golang.org/x/crypto/bcrypt"
)

const HubConfigExportVersion = 1

// HubConfigExport is the portable hub settings blob (no secrets).
type HubConfigExport struct {
	Version        int      `json:"version"`
	Endpoint       string   `json:"endpoint"`
	Subnet         string   `json:"subnet"`
	AdminUsername  string   `json:"admin_username"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

func (s *Settings) ToExport(adminUsername string) HubConfigExport {
	return HubConfigExport{
		Version:        HubConfigExportVersion,
		Endpoint:       s.Endpoint,
		Subnet:         s.WGSubnet,
		AdminUsername:  adminUsername,
		MTU:            s.MTU,
		StatusInterval: s.StatusInterval,
		UpstreamDNS:    append([]string(nil), s.UpstreamDNSOrDefault()...),
	}
}

// ValidateHubConfig checks import/setup fields. When requireEndpoint is true, endpoint must be set.
func ValidateHubConfig(cfg HubConfigExport, requireEndpoint bool) error {
	if cfg.Version != 0 && cfg.Version != HubConfigExportVersion {
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

	if _, err := ParseUpstreamDNS(cfg.UpstreamDNS); err != nil {
		return err
	}

	return nil
}

// ValidateListenPort checks the WireGuard listen port for setup and settings updates.
func ValidateListenPort(port int) error {
	return config.ValidateListenPort(port)
}

func NormalizeHubConfig(cfg HubConfigExport) HubConfigExport {
	out := cfg
	if out.Version == 0 {
		out.Version = HubConfigExportVersion
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
	dns, _ := ParseUpstreamDNS(out.UpstreamDNS)
	out.UpstreamDNS = dns
	return out
}

func (s *Store) UpdateMutableSettings(mtu, statusInterval, listenPort int, upstreamDNS []string) error {
	if err := ValidateListenPort(listenPort); err != nil {
		return err
	}
	settings, err := s.GetSettings()
	if err != nil {
		return err
	}
	draft := HubConfigExport{
		Version:        HubConfigExportVersion,
		Endpoint:       settings.Endpoint,
		Subnet:         settings.WGSubnet,
		AdminUsername:  config.DefaultAdminUsername,
		MTU:            mtu,
		StatusInterval: statusInterval,
		UpstreamDNS:    upstreamDNS,
	}
	if err := ValidateHubConfig(draft, true); err != nil {
		return err
	}
	norm := NormalizeHubConfig(draft)
	settings.MTU = norm.MTU
	settings.StatusInterval = norm.StatusInterval
	settings.ListenPort = listenPort
	settings.UpstreamDNS = norm.UpstreamDNS
	return s.UpdateSettings(settings)
}

func (s *Store) GetPrimaryAdmin() (*Admin, error) {
	var admin Admin
	if err := s.db.Order("id asc").First(&admin).Error; err != nil {
		return nil, err
	}
	return &admin, nil
}

func (s *Store) UpdateAdminPassword(adminID uint, newPassword string) error {
	if len(newPassword) < 8 {
		return fmt.Errorf("password must be at least 8 characters")
	}
	hash, err := bcrypt.GenerateFromPassword([]byte(newPassword), bcrypt.DefaultCost)
	if err != nil {
		return err
	}
	return s.db.Model(&Admin{}).Where("id = ?", adminID).Update("password_hash", string(hash)).Error
}
