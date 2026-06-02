package config

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
)

// DNSDomain is the private DNS suffix for peer hostnames ({name}.wirehub).
const DNSDomain = "wirehub"

const (
	DefaultSubnet         = "100.127.0.0/24"
	DefaultPort           = 8443 // Web UI / API (CLI --port)
	DefaultListenPort     = 8443 // WireGuard UDP port in hub and client configs
	DefaultBind           = "0.0.0.0"
	DefaultDataDir        = "./data"
	DefaultAdminUsername  = "admin"
	DefaultAdminPassword  = "admin"
	DefaultMTU            = 1420
	DefaultStatusInterval = 1
)

// DefaultUpstreamDNS are fallback public resolvers pushed to clients after the hub DNS IP.
var DefaultUpstreamDNS = []string{"1.2.4.8", "1.1.1.1"}

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
	port := flag.Int("port", DefaultPort, "TCP port for web UI and API (WireGuard port is configured at setup)")
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
