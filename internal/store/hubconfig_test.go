package store

import (
	"testing"
)

func TestValidateHubConfig(t *testing.T) {
	valid := HubConfigExport{
		Version:        HubConfigExportVersion,
		Endpoint:       "203.0.113.10",
		Subnet:         "100.127.0.0/24",
		AdminUsername:  "admin",
		MTU:            1420,
		StatusInterval: 5,
		UpstreamDNS:    []string{"1.1.1.1"},
	}
	if err := ValidateHubConfig(valid, true); err != nil {
		t.Fatalf("valid config: %v", err)
	}

	if err := ValidateHubConfig(HubConfigExport{
		Endpoint: "203.0.113.10",
		Subnet:   "not-a-cidr",
	}, true); err == nil {
		t.Fatal("expected subnet error")
	}

	if err := ValidateHubConfig(HubConfigExport{
		Endpoint: "203.0.113.10",
		Subnet:   "100.127.0.0/24",
		MTU:      500,
	}, true); err == nil {
		t.Fatal("expected mtu error")
	}
}

func TestValidateWireHubDatabaseInvalid(t *testing.T) {
	if err := ValidateWireHubDatabase("/nonexistent.db"); err == nil {
		t.Fatal("expected error for missing file")
	}
}

func TestNormalizeHubConfig(t *testing.T) {
	n := NormalizeHubConfig(HubConfigExport{
		Endpoint:    " 203.0.113.10 ",
		UpstreamDNS: nil,
	})
	if n.Subnet != "100.127.0.0/24" {
		t.Fatalf("subnet default: %q", n.Subnet)
	}
	if len(n.UpstreamDNS) == 0 {
		t.Fatal("expected default upstream dns")
	}
}
