package service

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
)

func testApp(t *testing.T) *App {
	t.Helper()
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	settings := &repo.Settings{
		WGSubnet: "100.127.0.0/24",
		HubIP:    "100.127.0.1",
		DNSIP:    "100.127.0.1",
	}
	if err := st.DB().Create(settings).Error; err != nil {
		t.Fatal(err)
	}
	return NewApp(st)
}

func TestCreateGroupLink_ReplacesDirection(t *testing.T) {
	a := testApp(t)

	g1, err := a.CreateGroup("a", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := a.CreateGroup("b", 100, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Create bidirectional link
	if err := a.CreateGroupLink(g1.ID, g2.ID, true); err != nil {
		t.Fatal(err)
	}

	links, err := a.Store().ListGroupLinks()
	if err != nil {
		t.Fatal(err)
	}
	if len(links) != 1 || !links[0].Bidirectional {
		t.Fatal("expected one bidirectional link")
	}

	// Replace with unidirectional link (same pair, different direction)
	// The old code had a no-op bug here; test that replacement works
	if err := a.CreateGroupLink(g2.ID, g1.ID, false); err != nil {
		t.Fatal(err)
	}

	links, err = a.Store().ListGroupLinks()
	if err != nil {
		t.Fatal(err)
	}
	if len(links) != 1 {
		t.Fatalf("expected 1 link after replacement, got %d", len(links))
	}
	if links[0].Bidirectional {
		t.Fatal("expected unidirectional after replacement")
	}
	// The unidirectional link should keep from→to order as given (g2 → g1)
	if links[0].FromGroupID != g2.ID || links[0].ToGroupID != g1.ID {
		t.Fatalf("expected link from %d→%d, got %+v", g2.ID, g1.ID, links[0])
	}
}

func TestCreateGroupLink_ReplacesToBidirectional(t *testing.T) {
	a := testApp(t)

	g1, err := a.CreateGroup("x", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := a.CreateGroup("y", 100, 0)
	if err != nil {
		t.Fatal(err)
	}

	// Create unidirectional link
	if err := a.CreateGroupLink(g1.ID, g2.ID, false); err != nil {
		t.Fatal(err)
	}

	// Replace with bidirectional
	if err := a.CreateGroupLink(g1.ID, g2.ID, true); err != nil {
		t.Fatal(err)
	}

	links, err := a.Store().ListGroupLinks()
	if err != nil {
		t.Fatal(err)
	}
	if len(links) != 1 {
		t.Fatalf("expected 1 link, got %d", len(links))
	}
	if !links[0].Bidirectional {
		t.Fatal("expected bidirectional link after replacement")
	}
}

func TestDeleteGroup_ClearsMapAllow(t *testing.T) {
	a := testApp(t)

	g, err := a.CreateGroup("admins", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := a.CreateGroup("users", 100, 0)
	if err != nil {
		t.Fatal(err)
	}

	detail, err := a.CreateServiceMap(repo.MapInput{
		Slug: "admin-svc", TargetHost: "10.0.0.1",
		AllowedGroups: []uint{g.ID, g2.ID},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Verify 2 allow rows
	groups, err := a.Store().ListMapGroupIDs(detail.ID)
	if err != nil || len(groups) != 2 {
		t.Fatalf("expected 2 groups, got %v err=%v", groups, err)
	}

	// Delete g2
	if err := a.DeleteGroup(g2.ID); err != nil {
		t.Fatal(err)
	}

	// Verify only g's allow row remains
	groups, err = a.Store().ListMapGroupIDs(detail.ID)
	if err != nil || len(groups) != 1 || groups[0] != g.ID {
		t.Fatalf("expected 1 group (g), got %v err=%v", groups, err)
	}
}

func TestCreateGroupLink_SelfLinkError(t *testing.T) {
	a := testApp(t)
	g, err := a.CreateGroup("self", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err := a.CreateGroupLink(g.ID, g.ID, false); err != ErrSelfLink {
		t.Fatalf("expected ErrSelfLink, got %v", err)
	}
}

func TestCreateGroupLink_NonexistentGroup(t *testing.T) {
	a := testApp(t)
	g, err := a.CreateGroup("exists", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err := a.CreateGroupLink(g.ID, 9999, false); err == nil {
		t.Fatal("expected error for nonexistent group")
	}
}
