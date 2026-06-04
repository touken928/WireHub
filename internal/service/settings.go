package service

import (
	"io"

	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/runtime"
)

// SettingsView is the settings page payload.
type SettingsView struct {
	Endpoint        string
	Subnet          string
	AdminUsername   string
	HubIP           string
	DNSIP           string
	DNSSuffix       string
	ListenPort      int
	ServerPublicKey string
	MTU             int
	StatusInterval  int
	UpstreamDNS     []string
}

// GetSettingsView loads settings and primary admin username for the UI.
func (a *App) GetSettingsView() (SettingsView, error) {
	settings, err := a.Store.GetSettings()
	if err != nil {
		return SettingsView{}, err
	}
	adminUsername := ""
	if admin, err := a.Store.GetPrimaryAdmin(); err == nil {
		adminUsername = admin.Username
	}
	return SettingsView{
		Endpoint:        settings.Endpoint,
		Subnet:          settings.WGSubnet,
		AdminUsername:   adminUsername,
		HubIP:           settings.HubIP,
		DNSIP:           settings.DNSIP,
		DNSSuffix:       settings.DNSSuffix,
		ListenPort:      settings.ListenPort,
		ServerPublicKey: settings.ServerPublicKey,
		MTU:             settings.MTU,
		StatusInterval:  settings.StatusInterval,
		UpstreamDNS:     settings.UpstreamDNSResolvers(),
	}, nil
}

// UpdateSettingsResult reports whether the VPN stack must be restarted.
type UpdateSettingsResult struct {
	RestartRequired bool
}

// UpdateMutableSettings persists MTU, status interval, and upstream DNS; refreshes runtime when needed.
func (a *App) UpdateMutableSettings(mtu, statusInterval int, upstream []string) (UpdateSettingsResult, error) {
	settings, err := a.Store.GetSettings()
	if err != nil {
		return UpdateSettingsResult{}, err
	}
	oldMTU := settings.MTU
	if err := a.Store.UpdateMutableSettings(mtu, statusInterval, upstream); err != nil {
		return UpdateSettingsResult{}, err
	}
	settings, err = a.Store.GetSettings()
	if err != nil {
		return UpdateSettingsResult{}, err
	}
	a.Hub.SetDNSUpstream(settings.UpstreamDNSResolvers())
	a.Hub.StopStatusPoller()
	a.Hub.StartStatusPoller(settings.StatusInterval)

	networkReload := settings.MTU != oldMTU
	net := a.Hub.NetworkRuntime()
	if networkReload && net != nil {
		if err := net.ReloadSettings(); err != nil {
			return UpdateSettingsResult{}, err
		}
	}
	return UpdateSettingsResult{RestartRequired: networkReload}, nil
}

// SetDNSUpstream updates upstream resolvers on the live DNS server when the stack is running.
func (h *Hub) SetDNSUpstream(upstream []string) {
	nc := h.NetworkRuntime()
	if s, ok := nc.(*runtime.Stack); ok && s != nil {
		s.SetDNSUpstream(upstream)
	}
}

// UpdateAdminPassword changes the logged-in admin password.
func (a *App) UpdateAdminPassword(adminID uint, newPassword string) error {
	return a.Store.UpdateAdminPassword(adminID, newPassword)
}

// GetAdminByUsername loads an admin account.
func (a *App) GetAdminByUsername(username string) (*repo.Admin, error) {
	return a.Store.GetAdminByUsername(username)
}

// ExportDatabase streams the SQLite file to w.
func (a *App) ExportDatabase(w io.Writer) error {
	return a.Store.ExportDatabase(w)
}

// DatabasePath returns the on-disk SQLite path.
func (a *App) DatabasePath() string {
	return a.Store.DatabasePath()
}
