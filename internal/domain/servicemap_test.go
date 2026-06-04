package domain

import "testing"

func TestValidateMapSlug(t *testing.T) {
	if _, err := ValidateMapSlug("hub"); err == nil {
		t.Fatal("hub slug reserved")
	}
	if got, err := ValidateMapSlug("db"); err != nil || got != "db" {
		t.Fatalf("db: %v %q", err, got)
	}
}

func TestValidateMapGroupIDs(t *testing.T) {
	if err := ValidateMapGroupIDs(nil); err == nil {
		t.Fatal("expected error for empty groups")
	}
	if err := ValidateMapGroupIDs([]uint{1, 2}); err != nil {
		t.Fatal(err)
	}
}
