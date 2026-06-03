package dns

import "testing"

func TestDNSSlug(t *testing.T) {
	tests := []struct {
		in   string
		want string
	}{
		{"hub", "hub"},
		{"www.hub", "hub"},
		{"www.touken", "touken"},
		{"touken", "touken"},
		{"WWW.Touken", "touken"},
		{"www", "www"},
	}
	for _, tc := range tests {
		if got := dnsSlug(tc.in); got != tc.want {
			t.Fatalf("dnsSlug(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}
