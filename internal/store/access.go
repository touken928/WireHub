package store

import (
	"fmt"
	"net/netip"
	"regexp"
	"strings"

	"github.com/touken928/wirehub/internal/hostname"
)

func ValidateHostname(name string) (string, error) {
	return hostname.Validate(name)
}

// ParseExcludeLines splits newline-delimited input into ordered exclude patterns.
// Lines starting with # are comments; blank lines are ignored.
func ParseExcludeLines(raw []string) []string {
	out := make([]string, 0, len(raw))
	for _, line := range raw {
		for _, part := range strings.Split(line, "\n") {
			part = strings.TrimSpace(part)
			if part == "" || strings.HasPrefix(part, "#") {
				continue
			}
			out = append(out, part)
		}
	}
	return out
}

func ParseExcludePattern(rule string) (pattern string, negated bool, err error) {
	rule = strings.TrimSpace(rule)
	if rule == "" {
		return "", false, fmt.Errorf("empty rule")
	}
	if strings.HasPrefix(rule, "!") {
		pattern = strings.TrimSpace(rule[1:])
		if pattern == "" {
			return "", false, fmt.Errorf("empty negation rule")
		}
		return pattern, true, nil
	}
	return rule, false, nil
}

// ValidateExcludePattern checks a hostname exclude pattern (exact or wildcard).
func ValidateExcludePattern(pattern string) error {
	pattern = strings.TrimSpace(pattern)
	if pattern == "" {
		return fmt.Errorf("empty pattern")
	}
	if strings.Contains(pattern, ".") {
		return fmt.Errorf("patterns must be hostnames only, without domain suffix (got %q)", pattern)
	}
	if addr, err := netip.ParseAddr(pattern); err == nil && addr.Is4() {
		return fmt.Errorf("patterns must be hostnames, not IP addresses (got %q)", pattern)
	}
	if strings.Contains(pattern, "*") {
		return validateWildcardPattern(pattern)
	}
	if _, err := hostname.Validate(pattern); err != nil {
		return fmt.Errorf("invalid hostname pattern %q: %w", pattern, err)
	}
	return nil
}

func validateWildcardPattern(pattern string) error {
	for _, r := range pattern {
		switch {
		case r >= 'a' && r <= 'z', r >= '0' && r <= '9', r == '-', r == '*':
		default:
			return fmt.Errorf("invalid character in wildcard pattern %q", pattern)
		}
	}
	return nil
}

var globRE = regexp.MustCompile(`[^*]+|\*`)

func globMatch(pattern, s string) bool {
	pattern = strings.ToLower(strings.TrimSpace(pattern))
	s = strings.ToLower(strings.TrimSpace(s))
	if pattern == "*" {
		return true
	}
	if !strings.Contains(pattern, "*") {
		return pattern == s
	}

	var re strings.Builder
	re.WriteString("^")
	for _, part := range globRE.FindAllString(pattern, -1) {
		if part == "*" {
			re.WriteString(".*")
			continue
		}
		re.WriteString(regexp.QuoteMeta(part))
	}
	re.WriteString("$")
	matched, err := regexp.MatchString(re.String(), s)
	return err == nil && matched
}

func peerMatchesPattern(peer Peer, pattern string) bool {
	name := strings.ToLower(peer.Name)
	if strings.Contains(pattern, "*") {
		return globMatch(pattern, name)
	}
	return strings.EqualFold(strings.TrimSpace(pattern), name)
}

// ResolveExcludeRules evaluates gitignore-style patterns and returns blocked peer IPs.
// Default is unrestricted; the last matching pattern wins. Use !prefix to re-allow.
func ResolveExcludeRules(peers []Peer, self Peer, lines []string) ([]string, error) {
	rules := ParseExcludeLines(lines)
	for _, rule := range rules {
		pattern, _, err := ParseExcludePattern(rule)
		if err != nil {
			return nil, err
		}
		if err := ValidateExcludePattern(pattern); err != nil {
			return nil, err
		}
		if !strings.Contains(pattern, "*") {
			if slug, err := hostname.Validate(pattern); err == nil && slug == self.Name {
				return nil, fmt.Errorf("cannot exclude own hostname")
			}
		}
	}

	blocked := make([]string, 0)
	seen := make(map[string]struct{})

	for _, p := range peers {
		if p.Name == self.Name || p.WGIP == "" {
			continue
		}

		var excluded *bool
		for _, rule := range rules {
			pattern, negated, err := ParseExcludePattern(rule)
			if err != nil {
				return nil, err
			}
			if !peerMatchesPattern(p, pattern) {
				continue
			}
			v := !negated
			excluded = &v
		}

		if excluded != nil && *excluded {
			if _, ok := seen[p.WGIP]; !ok {
				seen[p.WGIP] = struct{}{}
				blocked = append(blocked, p.WGIP)
			}
		}
	}
	return blocked, nil
}

// MigratePeerAccessDefaults clears legacy whitelist rules when upgrading from older schemas.
func (s *Store) MigratePeerAccessDefaults() error {
	if !s.db.Migrator().HasColumn("peers", "access_mode") {
		return nil
	}
	return s.db.Exec(`UPDATE peers SET access_hosts = '[]' WHERE access_mode = 'whitelist'`).Error
}
