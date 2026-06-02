package hostname

import (
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/touken928/wirehub/internal/config"
)

const maxLabelLen = 63

var reserved = map[string]struct{}{
	"hub": {},
	"dns": {},
	"www": {},
}

func Slug(name string) string {
	name = strings.ToLower(strings.TrimSpace(name))
	name = strings.ReplaceAll(name, " ", "-")
	var b strings.Builder
	for _, r := range name {
		if (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9') || r == '-' {
			b.WriteRune(r)
		}
	}
	result := b.String()
	if result == "" {
		return "host"
	}
	return result
}

func Validate(name string) (string, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return "", fmt.Errorf("hostname is required")
	}
	if utf8.RuneCountInString(name) > maxLabelLen {
		return "", fmt.Errorf("hostname too long (max %d characters)", maxLabelLen)
	}
	slug := Slug(name)
	if slug == "host" && name != "host" {
		return "", fmt.Errorf("invalid hostname: use letters, numbers, and hyphens only")
	}
	if strings.HasPrefix(slug, "-") || strings.HasSuffix(slug, "-") {
		return "", fmt.Errorf("hostname cannot start or end with a hyphen")
	}
	if strings.Contains(slug, "--") {
		return "", fmt.Errorf("hostname cannot contain consecutive hyphens")
	}
	if _, ok := reserved[slug]; ok {
		return "", fmt.Errorf("hostname %q is reserved", slug)
	}
	return slug, nil
}

func FQDN(slug string) string {
	if slug == "" {
		return config.DNSDomain
	}
	return fmt.Sprintf("%s.%s", slug, config.DNSDomain)
}

func HubFQDN() string {
	return config.DNSDomain
}
