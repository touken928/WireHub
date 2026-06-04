package config

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
)

// DNSDomain is the private DNS suffix ({label}.wirehub).
const DNSDomain = "wirehub"

// HubDNSLabel is the hostname label for the hub itself (hub.wirehub).
const HubDNSLabel = "hub"

const (
	DefaultSubnet         = "100.127.0.0/24"
	DefaultPort           = 8443 // CLI --port: WireGuard UDP listen + Web UI/API TCP (same number)
	DefaultEndpointPort   = 8443 // Default port in client configs (setup/settings; may differ with NAT)
	DefaultBind           = "0.0.0.0"
	DefaultDataDir        = "./data"
	DefaultAdminUsername  = "admin"
	DefaultAdminPassword  = "admin"
	DefaultMTU            = 1420
	DefaultStatusInterval = 1
)

// DefaultUpstreamDNS are default hub upstream resolvers (setup UI hint and recommended settings).
var DefaultUpstreamDNS = []string{"114.114.114.114", "1.1.1.1"}

// RuntimeConfig holds process-level settings from CLI flags.
// Persistent hub settings (endpoint, subnet, admin, MTU, etc.) live in the database after setup.
type RuntimeConfig struct {
	Bind         string
	Port         int
	DataDir      string
	ListenAddr   string
	DatabasePath string
	JWTSecret    string
}

// ParseFlags parses CLI flags and returns runtime configuration.
func ParseFlags() (*RuntimeConfig, error) {
	port := flag.Int("port", DefaultPort, "TCP port for web UI/API and UDP port for WireGuard (same number)")
	bind := flag.String("bind", DefaultBind, "IP address to bind the web UI")
	dataDir := flag.String("data-dir", DefaultDataDir, "data directory (SQLite DB, JWT secret)")
	flag.Parse()

	if *port <= 0 || *port > 65535 {
		return nil, fmt.Errorf("port must be between 1 and 65535")
	}

	dataDirPath := *dataDir
	if dataDirPath == "" {
		dataDirPath = DefaultDataDir
	}

	if err := os.MkdirAll(dataDirPath, 0o755); err != nil {
		return nil, fmt.Errorf("create data dir: %w", err)
	}

	jwtSecret, err := loadOrCreateJWTSecret(dataDirPath)
	if err != nil {
		return nil, err
	}

	bindAddr := *bind
	if bindAddr == "" {
		bindAddr = DefaultBind
	}

	return &RuntimeConfig{
		Bind:         bindAddr,
		Port:         *port,
		DataDir:      dataDirPath,
		ListenAddr:   fmt.Sprintf("%s:%d", bindAddr, *port),
		DatabasePath: filepath.Join(dataDirPath, "wirehub.db"),
		JWTSecret:    jwtSecret,
	}, nil
}
