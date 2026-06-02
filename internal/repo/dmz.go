package repo

import (
	"fmt"

	"github.com/touken928/wirehub/internal/domain"
	"gorm.io/gorm"
)

const portForwardDMZRowID uint = 1

// PortForwardDMZ forwards all hub VPN ports (same port on target) except reserved and explicit forwards.
type PortForwardDMZ struct {
	ID         uint   `gorm:"primaryKey" json:"id"`
	TargetHost string `gorm:"not null" json:"target_host"`
	Enabled    bool   `gorm:"not null" json:"enabled"`
}

type PortForwardDMZInput struct {
	TargetHost string
	Enabled    bool
}

func (s *Store) GetPortForwardDMZ() (*PortForwardDMZ, error) {
	var row PortForwardDMZ
	err := s.db.First(&row, portForwardDMZRowID).Error
	if err == gorm.ErrRecordNotFound {
		return &PortForwardDMZ{ID: portForwardDMZRowID, Enabled: false}, nil
	}
	if err != nil {
		return nil, err
	}
	return &row, nil
}

func (s *Store) UpsertPortForwardDMZ(in PortForwardDMZInput) (*PortForwardDMZ, error) {
	targetHost, err := domain.ValidateForwardTargetHost(in.TargetHost)
	if err != nil {
		return nil, err
	}
	if in.Enabled && targetHost == "" {
		return nil, fmt.Errorf("target host is required when DMZ is enabled")
	}
	row := PortForwardDMZ{
		ID:         portForwardDMZRowID,
		TargetHost: targetHost,
		Enabled:    in.Enabled,
	}
	if err := s.db.Save(&row).Error; err != nil {
		return nil, err
	}
	return &row, nil
}

func (s *Store) ensurePortForwardDMZRow() error {
	var count int64
	if err := s.db.Model(&PortForwardDMZ{}).Count(&count).Error; err != nil {
		return err
	}
	if count > 0 {
		return nil
	}
	return s.db.Create(&PortForwardDMZ{ID: portForwardDMZRowID, Enabled: false}).Error
}
