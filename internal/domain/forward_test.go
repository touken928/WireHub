package domain

import "testing"

func TestValidateForwardTargetHost(t *testing.T) {
	tests := []struct {
		in      string
		want    string
		wantErr bool
	}{
		{"10.0.0.2", "10.0.0.2", false},
		{"app.wirehub", "app.wirehub", false},
		{"service.example.com", "service.example.com", false},
		{"app", "", true},
		{"", "", true},
	}
	for _, tc := range tests {
		got, err := ValidateForwardTargetHost(tc.in)
		if tc.wantErr {
			if err == nil {
				t.Errorf("%q: want error", tc.in)
			}
			continue
		}
		if err != nil {
			t.Errorf("%q: %v", tc.in, err)
			continue
		}
		if got != tc.want {
			t.Errorf("%q: got %q want %q", tc.in, got, tc.want)
		}
	}
}
