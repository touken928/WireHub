package service

import (
	"errors"

	"github.com/touken928/wirehub/internal/domain/forward"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/core"
	"gorm.io/gorm"
)

// ForwardList bundles port-forward rules with hub addressing hints.
type ForwardList struct {
	Rules   []repo.PortForward
	HubIP   string
	HubPort int
}

// HubTunnelWebPort is the tunnel Web listen port on the hub VPN IP.
func HubTunnelWebPort() int {
	return core.HubTunnelWebPort
}

// ListPortForwards returns all forward rules and hub IP for the UI.
func (a *App) ListPortForwards() (ForwardList, error) {
	rules, err := a.Store.ListPortForwards()
	if err != nil {
		return ForwardList{}, err
	}
	settings, _ := a.Store.GetSettings()
	hubIP := ""
	if settings != nil {
		hubIP = settings.HubIP
	}
	return ForwardList{Rules: rules, HubIP: hubIP, HubPort: HubTunnelWebPort()}, nil
}

// CreatePortForward adds a forward rule and syncs the dataplane.
func (a *App) CreatePortForward(in repo.PortForwardInput) (*repo.PortForward, error) {
	rule, err := a.Store.CreatePortForward(HubTunnelWebPort(), in)
	if err != nil {
		return nil, err
	}
	if err := a.Hub.SyncPortForwards(); err != nil {
		return nil, err
	}
	return rule, nil
}

// UpdatePortForward updates a forward rule and syncs the dataplane.
func (a *App) UpdatePortForward(id uint, in repo.PortForwardInput) (*repo.PortForward, error) {
	rule, err := a.Store.UpdatePortForward(id, HubTunnelWebPort(), in)
	if err != nil {
		return nil, err
	}
	if err := a.Hub.SyncPortForwards(); err != nil {
		return nil, err
	}
	return rule, nil
}

// DeletePortForward removes a forward rule and syncs the dataplane.
func (a *App) DeletePortForward(id uint) error {
	if err := a.Store.DeletePortForward(id); err != nil {
		return err
	}
	return a.Hub.SyncPortForwards()
}

// ForwardDisplayTarget formats a target host:port for the UI.
func ForwardDisplayTarget(host string, port int) string {
	return forward.ForwardDisplayTarget(host, port)
}

// ForwardErrKind classifies port-forward errors for HTTP mapping.
type ForwardErrKind int

const (
	ForwardErrOther ForwardErrKind = iota
	ForwardErrConflict
	ForwardErrNotFound
)

// ClassifyForwardErr maps store errors to HTTP-friendly kinds.
func ClassifyForwardErr(err error) ForwardErrKind {
	if errors.Is(err, repo.ErrPortForwardConflict) {
		return ForwardErrConflict
	}
	if errors.Is(err, gorm.ErrRecordNotFound) {
		return ForwardErrNotFound
	}
	return ForwardErrOther
}
