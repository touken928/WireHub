package dns

import "testing"

func TestDNSSlug(t *testing.T) {
	tests := []struct {
		in   string
		want string
	}{
		{"www", ""},
		{"www.touken", "touken"},
		{"touken", "touken"},
		{"WWW.Touken", "touken"},
	}
	for _, tc := range tests {
		if got := dnsSlug(tc.in); got != tc.want {
			t.Fatalf("dnsSlug(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}
