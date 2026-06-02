package repo

import (
	"fmt"

	"github.com/touken928/wirehub/internal/password"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain"
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

	draft := domain.HubConfig{
		Endpoint:       in.Endpoint,
		Subnet:         in.Subnet,
		AdminUsername:  in.AdminUsername,
		MTU:            in.MTU,
		StatusInterval: in.StatusInterval,
		UpstreamDNS:    in.UpstreamDNS,
	}
	if err := domain.ValidateHubConfig(draft, true); err != nil {
		return err
	}
	norm := domain.NormalizeHubConfig(draft)

	if in.AdminPassword == "" {
		return fmt.Errorf("admin password is required")
	}
	hash, err := password.Hash(in.AdminPassword)
	if err != nil {
		return err
	}

	listenPort := in.ListenPort
	if listenPort == 0 {
		listenPort = config.DefaultEndpointPort
	}
	if err := config.ValidateEndpointPort(listenPort); err != nil {
		return err
	}

	if in.ServerPrivateKey == "" || in.ServerPublicKey == "" {
		return fmt.Errorf("server keys are required")
	}

	hubIP, err := config.FirstHostIP(norm.Subnet)
	if err != nil {
		return fmt.Errorf("subnet: %w", err)
	}

	return s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Create(&Admin{Username: norm.AdminUsername, PasswordHash: hash}).Error; err != nil {
			return err
		}
		settings := &Settings{
			Endpoint:         norm.Endpoint,
			ListenPort:       listenPort,
			WGSubnet:         norm.Subnet,
			HubIP:            hubIP,
			DNSIP:            hubIP,
			DNSSuffix:        config.DNSDomain,
			ServerPrivateKey: in.ServerPrivateKey,
			ServerPublicKey:  in.ServerPublicKey,
			MTU:              norm.MTU,
			StatusInterval:   norm.StatusInterval,
			UpstreamDNS:      norm.UpstreamDNS,
		}
		if err := tx.Create(settings).Error; err != nil {
			return err
		}
		return tx.Create(&PeerGroup{Name: "default", PosX: 0, PosY: 0}).Error
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
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&GroupLink{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&PeerGroup{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&Admin{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&Settings{}).Error; err != nil {
			return err
		}
		if err := tx.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&PortForward{}).Error; err != nil {
			return err
		}
		return nil
	})
}
