package config

import "testing"

func TestValidateListenPort(t *testing.T) {
	if err := ValidateListenPort(51820); err != nil {
		t.Fatal(err)
	}
	if err := ValidateListenPort(0); err == nil {
		t.Fatal("expected error for port 0")
	}
	if err := ValidateListenPort(70000); err == nil {
		t.Fatal("expected error for port 70000")
	}
}
