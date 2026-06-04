package domain

import (
	"fmt"
	"strings"

	"github.com/touken928/wirehub/internal/config"
)

// ValidateMapSlug checks a service-map DNS label ({slug}.wirehub).
func ValidateMapSlug(slug string) (string, error) {
	slug, err := ValidateHostname(slug)
	if err != nil {
		return "", err
	}
	if slug == config.HubDNSLabel {
		return "", fmt.Errorf("%q is reserved", slug)
	}
	return slug, nil
}

// ValidateMapTargetHost accepts FQDN or IPv4 targets (same rules as port forward).
func ValidateMapTargetHost(host string) (string, error) {
	return ValidateForwardTargetHost(host)
}

// ValidateMapGroupIDs requires at least one allowed group (default deny).
func ValidateMapGroupIDs(ids []uint) error {
	if len(ids) == 0 {
		return fmt.Errorf("at least one allowed group is required")
	}
	seen := make(map[uint]struct{}, len(ids))
	for _, id := range ids {
		if id == 0 {
			return fmt.Errorf("invalid group id")
		}
		if _, ok := seen[id]; ok {
			continue
		}
		seen[id] = struct{}{}
	}
	return nil
}

// MapDisplayName returns a trimmed display name or the slug.
func MapDisplayName(name, slug string) string {
	name = strings.TrimSpace(name)
	if name != "" {
		return name
	}
	return slug
}

// MapFQDN returns the authoritative DNS name for a map slug.
func MapFQDN(slug string) string {
	return PeerFQDN(slug)
}
