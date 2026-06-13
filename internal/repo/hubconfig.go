package repo

import (
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/hub"
	"gorm.io/gorm"
)

func (s *Settings) ToHubConfig(adminUsername string) hub.HubConfig {
	return hub.HubConfig{
		Version:        hub.HubConfigVersion,
		Endpoint:       s.Endpoint,
		Subnet:         s.WGSubnet,
		AdminUsername:  adminUsername,
		MTU:            s.MTU,
		StatusInterval: s.StatusInterval,
		UpstreamDNS:    append([]string(nil), s.UpstreamDNS...),
	}
}

func (s *Store) UpdateMutableSettings(mtu, statusInterval int, upstreamDNS []string) error {
	settings, err := s.GetSettings()
	if err != nil {
		return err
	}
	draft := hub.HubConfig{
		Version:        hub.HubConfigVersion,
		Endpoint:       settings.Endpoint,
		Subnet:         settings.WGSubnet,
		AdminUsername:  config.DefaultAdminUsername,
		MTU:            mtu,
		StatusInterval: statusInterval,
		UpstreamDNS:    upstreamDNS,
	}
	if err := hub.ValidateHubConfig(draft, true); err != nil {
		return err
	}
	norm := hub.NormalizeHubConfig(draft)
	settings.MTU = norm.MTU
	settings.StatusInterval = norm.StatusInterval
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
	hash, err := HashPassword(newPassword)
	if err != nil {
		return err
	}
	return s.db.Model(&Admin{}).Where("id = ?", adminID).Updates(map[string]interface{}{
		"password_hash": hash,
		"token_version": gorm.Expr("token_version + 1"),
	}).Error
}
