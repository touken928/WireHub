package store

import (
	"fmt"
	"strings"

	"github.com/touken928/wirehub/internal/config"
	"golang.org/x/crypto/bcrypt"
	"gorm.io/gorm"
)

// SetupInput is submitted once during first-run setup.
type SetupInput struct {
	Endpoint         string
	Subnet           string
	AdminUsername    string
	AdminPassword    string
	MTU              int
	StatusInterval   int
	ListenPort       int
	ServerPrivateKey string
	ServerPublicKey  string
	UpstreamDNS      []string
}

func (s *Store) IsConfigured() (bool, error) {
	var adminCount, settingsCount int64
	if err := s.db.Model(&Admin{}).Count(&adminCount).Error; err != nil {
		return false, err
	}
	if err := s.db.Model(&Settings{}).Count(&settingsCount).Error; err != nil {
		return false, err
	}
	return adminCount > 0 && settingsCount > 0, nil
}

func (s *Store) Setup(in SetupInput) error {
	configured, err := s.IsConfigured()
	if err != nil {
		return err
	}
	if configured {
		return fmt.Errorf("already configured")
	}

	endpoint := strings.TrimSpace(in.Endpoint)
	if endpoint == "" {
		return fmt.Errorf("endpoint is required")
	}

	subnet := strings.TrimSpace(in.Subnet)
	if subnet == "" {
		subnet = config.DefaultSubnet
	}
	hubIP, err := config.FirstHostIP(subnet)
	if err != nil {
		return fmt.Errorf("subnet: %w", err)
	}

	username := strings.TrimSpace(in.AdminUsername)
	if username == "" {
		username = config.DefaultAdminUsername
	}
	password := in.AdminPassword
	if password == "" {
		return fmt.Errorf("admin password is required")
	}

	mtu := in.MTU
	if mtu == 0 {
		mtu = config.DefaultMTU
	}
	statusInterval := in.StatusInterval
	if statusInterval == 0 {
		statusInterval = config.DefaultStatusInterval
	}
	listenPort := in.ListenPort
	if listenPort == 0 {
		listenPort = config.DefaultPort
	}

	if in.ServerPrivateKey == "" || in.ServerPublicKey == "" {
		return fmt.Errorf("server keys are required")
	}

	upstreamDNS, err := ParseUpstreamDNS(in.UpstreamDNS)
	if err != nil {
		return err
	}

	hash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return err
	}

	return s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Create(&Admin{Username: username, PasswordHash: string(hash)}).Error; err != nil {
			return err
		}
		settings := &Settings{
			Endpoint:         endpoint,
			ListenPort:       listenPort,
			WGSubnet:         subnet,
			HubIP:            hubIP,
			DNSIP:            hubIP,
			DNSSuffix:        config.DNSDomain,
			ServerPrivateKey: in.ServerPrivateKey,
			ServerPublicKey:  in.ServerPublicKey,
			MTU:              mtu,
			StatusInterval:   statusInterval,
			UpstreamDNS:      upstreamDNS,
		}
		return tx.Create(settings).Error
	})
}

func (s *Store) ResetAll() error {
	return s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&DNSRecord{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&Peer{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&Admin{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&Settings{}).Error; err != nil {
			return err
		}
		return nil
	})
}
