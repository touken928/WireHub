package hostname

import "testing"

func TestValidate(t *testing.T) {
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
		got, err := Validate(tc.in)
		if tc.isErr {
			if err == nil {
				t.Fatalf("Validate(%q) expected error", tc.in)
			}
			continue
		}
		if err != nil {
			t.Fatalf("Validate(%q): %v", tc.in, err)
		}
		if got != tc.want {
			t.Fatalf("Validate(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}
