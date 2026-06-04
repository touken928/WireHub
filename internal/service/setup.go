package service

import (
	"errors"
	"os"
	"path/filepath"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// SetupDefaults are shown on the setup page before configuration.
type SetupDefaults struct {
	Subnet         string
	AdminUsername  string
	ListenPort     int
	MTU            int
	StatusInterval int
	UpstreamDNS    []string
}

// SetupStatus reports whether the hub database is initialized.
func (a *App) SetupStatus() (configured bool, defaults SetupDefaults, err error) {
	configured, err = a.Store.IsConfigured()
	if err != nil {
		return false, SetupDefaults{}, err
	}
	return configured, SetupDefaults{
		Subnet:         config.DefaultSubnet,
		AdminUsername:  config.DefaultAdminUsername,
		ListenPort:     config.DefaultEndpointPort,
		MTU:            config.DefaultMTU,
		StatusInterval: config.DefaultStatusInterval,
		UpstreamDNS:    append([]string(nil), config.DefaultUpstreamDNS...),
	}, nil
}

// SetupInput is the first-time hub configuration payload.
type SetupInput struct {
	Endpoint       string
	Subnet         string
	AdminUsername  string
	AdminPassword  string
	ListenPort     int
	MTU            int
	StatusInterval int
	UpstreamDNS    []string
}

// Setup initializes the hub database and starts the network stack when available.
func (a *App) Setup(in SetupInput) error {
	configured, err := a.Store.IsConfigured()
	if err != nil {
		return err
	}
	if configured {
		return ErrAlreadyConfigured
	}
	listenPort := in.ListenPort
	if listenPort == 0 {
		listenPort = config.DefaultEndpointPort
	}
	priv, pub, err := tunnel.GenerateKeyPair()
	if err != nil {
		return err
	}
	if err := a.Store.Setup(repo.SetupInput{
		Endpoint:         in.Endpoint,
		Subnet:           in.Subnet,
		AdminUsername:    in.AdminUsername,
		AdminPassword:    in.AdminPassword,
		MTU:              in.MTU,
		StatusInterval:   in.StatusInterval,
		ListenPort:       listenPort,
		ServerPrivateKey: priv,
		ServerPublicKey:  pub,
		UpstreamDNS:      in.UpstreamDNS,
	}); err != nil {
		return err
	}
	return a.startNetworkAfterSetup()
}

func (a *App) startNetworkAfterSetup() error {
	net := a.Hub.NetworkRuntime()
	if net == nil {
		return ErrNetworkUnavailable
	}
	bundle, err := a.LoadSyncBundle()
	if err != nil {
		_ = a.Store.ResetAll()
		return err
	}
	if err := net.Start(bundle); err != nil {
		_ = a.Store.ResetAll()
		return err
	}
	return nil
}

// ImportDatabase replaces the SQLite file before the hub is configured.
func (a *App) ImportDatabase(tmpPath string) error {
	configured, err := a.Store.IsConfigured()
	if err != nil {
		return err
	}
	if configured {
		return ErrImportWhenConfigured
	}
	if err := a.Store.ImportDatabase(tmpPath); err != nil {
		return err
	}
	net := a.Hub.NetworkRuntime()
	if net == nil {
		return nil
	}
	bundle, err := a.LoadSyncBundle()
	if err != nil {
		return err
	}
	return net.Start(bundle)
}

// PrepareDBUploadDir ensures the data directory exists for setup import.
func (a *App) PrepareDBUploadDir() (dataDir string, err error) {
	dataDir = filepath.Dir(a.Store.DatabasePath())
	err = os.MkdirAll(dataDir, 0o755)
	return dataDir, err
}

// Reset stops the network stack and clears all hub data after password verification.
func (a *App) Reset() error {
	net := a.Hub.NetworkRuntime()
	if net == nil {
		return ErrNetworkUnavailable
	}
	if err := net.Stop(); err != nil {
		return err
	}
	return a.Store.ResetAll()
}

// IsConfigured reports whether setup has completed.
func (a *App) IsConfigured() (bool, error) {
	return a.Store.IsConfigured()
}

var (
	ErrAlreadyConfigured    = errors.New("already configured")
	ErrImportWhenConfigured = errors.New("hub is already configured; reset before importing a database")
)
