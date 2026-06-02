package config

import "fmt"

// ValidateEndpointPort checks the port written to peer configs (Endpoint host:port), not the hub bind port.
func ValidateEndpointPort(port int) error {
	if port < 1 || port > 65535 {
		return fmt.Errorf("endpoint port must be between 1 and 65535")
	}
	return nil
}
