package domain

import "testing"

func TestValidateHostname(t *testing.T) {
	tests := []struct {
		in    string
		want  string
		isErr bool
	}{
		{"laptop", "laptop", false},
		{"www", "", true},
		{"My Server", "my-server", false},
		{"", "", true},
		{"---", "", true},
		{"-abc", "", true},
		{"abc-", "", true},
		{"a--b", "", true},
		{"hub", "", true},
		{"@#$", "", true},
	}
	for _, tc := range tests {
		got, err := ValidateHostname(tc.in)
		if tc.isErr {
			if err == nil {
				t.Fatalf("ValidateHostname(%q) expected error", tc.in)
			}
			continue
		}
		if err != nil {
			t.Fatalf("ValidateHostname(%q): %v", tc.in, err)
		}
		if got != tc.want {
			t.Fatalf("ValidateHostname(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestHubFQDN(t *testing.T) {
	if got := HubFQDN(); got != "hub.wirehub" {
		t.Fatalf("HubFQDN() = %q, want hub.wirehub", got)
	}
	if got := PeerFQDN("alice"); got != "alice.wirehub" {
		t.Fatalf("PeerFQDN(alice) = %q, want alice.wirehub", got)
	}
}
