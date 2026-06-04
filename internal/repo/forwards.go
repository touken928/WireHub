package repo

import (
	"errors"
	"fmt"
	"strings"

	"github.com/touken928/wirehub/internal/domain"
)

var ErrPortForwardConflict = errors.New("listen port and protocol already in use")

type PortForwardInput struct {
	Name       string
	ListenPort int
	Protocol   string
	TargetHost string
	TargetPort int
}

func (s *Store) ListPortForwards() ([]PortForward, error) {
	var rules []PortForward
	err := s.db.Order("listen_port asc, protocol asc").Find(&rules).Error
	return rules, err
}

func (s *Store) GetPortForward(id uint) (*PortForward, error) {
	var rule PortForward
	if err := s.db.First(&rule, id).Error; err != nil {
		return nil, err
	}
	return &rule, nil
}

func (s *Store) CreatePortForward(hubTunnelWebPort int, in PortForwardInput) (*PortForward, error) {
	rule, err := normalizePortForward(in, hubTunnelWebPort)
	if err != nil {
		return nil, err
	}
	if taken, err := s.IsHubListenPortUsed(rule.ListenPort); err != nil {
		return nil, err
	} else if taken {
		return nil, fmt.Errorf("listen port %d is already in use", rule.ListenPort)
	}
	if err := s.db.Create(rule).Error; err != nil {
		if strings.Contains(err.Error(), "UNIQUE") || strings.Contains(err.Error(), "unique") {
			return nil, ErrPortForwardConflict
		}
		return nil, err
	}
	return rule, nil
}

func (s *Store) UpdatePortForward(id uint, hubTunnelWebPort int, in PortForwardInput) (*PortForward, error) {
	rule, err := s.GetPortForward(id)
	if err != nil {
		return nil, err
	}
	updated, err := normalizePortForward(in, hubTunnelWebPort)
	if err != nil {
		return nil, err
	}
	if updated.ListenPort != rule.ListenPort {
		if taken, err := s.isHubListenPortUsed(updated.ListenPort, rule.ID); err != nil {
			return nil, err
		} else if taken {
			return nil, fmt.Errorf("listen port %d is already in use", updated.ListenPort)
		}
	}
	updated.ID = rule.ID
	if err := s.db.Save(updated).Error; err != nil {
		if strings.Contains(err.Error(), "UNIQUE") || strings.Contains(err.Error(), "unique") {
			return nil, ErrPortForwardConflict
		}
		return nil, err
	}
	return updated, nil
}

func (s *Store) DeletePortForward(id uint) error {
	return s.db.Delete(&PortForward{}, id).Error
}

func normalizePortForward(in PortForwardInput, hubTunnelWebPort int) (*PortForward, error) {
	proto, err := domain.ValidateForwardProtocol(in.Protocol)
	if err != nil {
		return nil, err
	}
	if err := domain.ValidateForwardListenPort(in.ListenPort, hubTunnelWebPort, proto); err != nil {
		return nil, err
	}
	if err := domain.ValidateForwardPort(in.TargetPort, "target port"); err != nil {
		return nil, err
	}
	targetHost, err := domain.ValidateForwardTargetHost(in.TargetHost)
	if err != nil {
		return nil, err
	}
	name := strings.TrimSpace(in.Name)
	if len(name) > 64 {
		return nil, fmt.Errorf("name must be at most 64 characters")
	}
	return &PortForward{
		Name:       name,
		ListenPort: in.ListenPort,
		Protocol:   proto,
		TargetHost: targetHost,
		TargetPort: in.TargetPort,
	}, nil
}
