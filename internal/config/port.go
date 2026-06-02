package config

import "fmt"

// ValidateListenPort checks the WireGuard port stored in hub settings and client configs.
func ValidateListenPort(port int) error {
	if port < 1 || port > 65535 {
		return fmt.Errorf("wireguard port must be between 1 and 65535")
	}
	return nil
}
