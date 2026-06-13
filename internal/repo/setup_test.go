package repo

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
)

func TestResetAll_ClearsServiceMapsAndAllows(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	settings := &Settings{
		WGSubnet: "100.127.0.0/24",
		HubIP:    "100.127.0.1",
		DNSIP:    "100.127.0.1",
	}
	if err := st.db.Create(settings).Error; err != nil {
		t.Fatal(err)
	}

	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Create a service map with allowed groups
	detail, err := st.CreateServiceMap(MapInput{
		Slug: "svc", TargetHost: "10.0.0.1", AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Verify the map and allow rows exist
	maps, err := st.ListServiceMaps()
	if err != nil || len(maps) != 1 {
		t.Fatalf("expected 1 map before reset, got %d err=%v", len(maps), err)
	}
	groups, err := st.ListMapGroupIDs(detail.ID)
	if err != nil || len(groups) != 1 {
		t.Fatalf("expected 1 group allow, got %d err=%v", len(groups), err)
	}

	// Reset
	if err := st.ResetAll(); err != nil {
		t.Fatal(err)
	}

	// Verify maps are gone
	maps, err = st.ListServiceMaps()
	if err != nil || len(maps) != 0 {
		t.Fatalf("expected 0 maps after reset, got %d err=%v", len(maps), err)
	}

	// Verify group allows are gone
	groups, err = st.ListMapGroupIDs(detail.ID)
	if err != nil || len(groups) != 0 {
		t.Fatalf("expected 0 group allows after reset, got %d err=%v", len(groups), err)
	}

	// Verify groups are also gone
	allGroups, err := st.ListGroups()
	if err != nil || len(allGroups) != 0 {
		t.Fatalf("expected 0 groups after reset, got %d err=%v", len(allGroups), err)
	}
}

func TestResetAll_CanUseStoreAfterReset(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	settings := &Settings{
		WGSubnet: "100.127.0.0/24",
		HubIP:    "100.127.0.1",
		DNSIP:    "100.127.0.1",
	}
	if err := st.db.Create(settings).Error; err != nil {
		t.Fatal(err)
	}

	g, err := st.CreateGroup("g", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	_, err = st.CreateServiceMap(MapInput{
		Slug: "svc", TargetHost: "10.0.0.1", AllowedGroups: []uint{g.ID},
	})
	if err != nil {
		t.Fatal(err)
	}

	if err := st.ResetAll(); err != nil {
		t.Fatal(err)
	}

	// After reset, settings and other tables are gone but the store is open.
	// Re-create settings (as a fresh setup would) to verify the store is usable.
	newSettings := &Settings{
		WGSubnet: "10.0.0.0/24",
		HubIP:    "10.0.0.1",
		DNSIP:    "10.0.0.1",
	}
	if err := st.db.Create(newSettings).Error; err != nil {
		t.Fatal(err)
	}
	g2, err := st.CreateGroup("new-group", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	_, err = st.CreateServiceMap(MapInput{
		Slug: "new-svc", TargetHost: "10.0.0.2", AllowedGroups: []uint{g2.ID},
	})
	if err != nil {
		t.Fatal(err)
	}
}
