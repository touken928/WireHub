package repo

import "testing"

func TestValidateWireHubDatabaseInvalid(t *testing.T) {
	if err := ValidateWireHubDatabase("/nonexistent.db"); err == nil {
		t.Fatal("expected error for missing file")
	}
}
