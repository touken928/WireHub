package config

import "testing"

func TestValidateEndpointPort(t *testing.T) {
	if err := ValidateEndpointPort(51820); err != nil {
		t.Fatal(err)
	}
	if err := ValidateEndpointPort(0); err == nil {
		t.Fatal("expected error for port 0")
	}
	if err := ValidateEndpointPort(70000); err == nil {
		t.Fatal("expected error for port 70000")
	}
}
