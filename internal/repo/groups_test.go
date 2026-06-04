package repo

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
)

func TestCreateGroup_DefaultAllowIntraGroup(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}

	g, err := st.CreateGroup("isolated-candidates", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	if !g.AllowIntraGroup {
		t.Fatal("new group should allow intra-group peer traffic by default")
	}

	got, err := st.GetGroup(g.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !got.AllowIntraGroup {
		t.Fatal("persisted group should allow intra-group by default")
	}
}

func TestUpdateGroup_AllowIntraGroup(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}

	g, err := st.CreateGroup("team", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g.AllowIntraGroup = false
	if err := st.UpdateGroup(g); err != nil {
		t.Fatal(err)
	}

	got, err := st.GetGroup(g.ID)
	if err != nil {
		t.Fatal(err)
	}
	if got.AllowIntraGroup {
		t.Fatal("expected allow_intra_group=false after update")
	}
}

func TestRenameGroup(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}

	g, err := st.CreateGroup("team", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := st.CreateGroup("other", 100, 0); err != nil {
		t.Fatal(err)
	}

	renamed, err := st.RenameGroup(g.ID, "ops")
	if err != nil {
		t.Fatal(err)
	}
	if renamed.Name != "ops" {
		t.Fatalf("expected ops, got %q", renamed.Name)
	}

	if _, err := st.RenameGroup(g.ID, "other"); err == nil {
		t.Fatal("expected duplicate name error")
	}
}

func TestGroupLinkUniquePair(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}

	g1, err := st.CreateGroup("a", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := st.CreateGroup("b", 100, 0)
	if err != nil {
		t.Fatal(err)
	}

	if err := st.UpsertGroupLink(g1.ID, g2.ID, true); err != nil {
		t.Fatal(err)
	}
	if err := st.UpsertGroupLink(g2.ID, g1.ID, true); err != nil {
		t.Fatal(err)
	}

	links, err := st.ListGroupLinks()
	if err != nil {
		t.Fatal(err)
	}
	if len(links) != 1 {
		t.Fatalf("expected 1 link, got %d", len(links))
	}
	if links[0].FromGroupID != g1.ID || links[0].ToGroupID != g2.ID {
		t.Fatalf("unexpected link pair: %+v", links[0])
	}
	if !links[0].Bidirectional {
		t.Fatal("expected bidirectional link")
	}

	exists, err := st.HasGroupLink(g2.ID, g1.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !exists {
		t.Fatal("expected link to exist")
	}
}

func TestUnidirectionalLinkPreservesDirection(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}

	g1, err := st.CreateGroup("from", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := st.CreateGroup("to", 100, 0)
	if err != nil {
		t.Fatal(err)
	}

	if err := st.UpsertGroupLink(g2.ID, g1.ID, false); err != nil {
		t.Fatal(err)
	}
	links, err := st.ListGroupLinks()
	if err != nil {
		t.Fatal(err)
	}
	if len(links) != 1 {
		t.Fatalf("expected 1 link, got %d", len(links))
	}
	if links[0].Bidirectional {
		t.Fatal("expected unidirectional link")
	}
	if links[0].FromGroupID != g2.ID || links[0].ToGroupID != g1.ID {
		t.Fatalf("unexpected direction: %+v", links[0])
	}
}

func TestHasGroupLink_AnyDirectionBetweenPair(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	g1, err := st.CreateGroup("a", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := st.CreateGroup("b", 100, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err := st.UpsertGroupLink(g1.ID, g2.ID, false); err != nil {
		t.Fatal(err)
	}
	for _, pair := range [][2]uint{{g1.ID, g2.ID}, {g2.ID, g1.ID}} {
		ok, err := st.HasGroupLink(pair[0], pair[1])
		if err != nil {
			t.Fatal(err)
		}
		if !ok {
			t.Fatalf("expected link between %d and %d", pair[0], pair[1])
		}
	}
}

func TestGroupLinkAtMostOneBetweenPair(t *testing.T) {
	dir := t.TempDir()
	st, err := New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	g1, err := st.CreateGroup("a", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	g2, err := st.CreateGroup("b", 100, 0)
	if err != nil {
		t.Fatal(err)
	}

	assertOneLink := func(t *testing.T) {
		t.Helper()
		links, err := st.ListGroupLinks()
		if err != nil {
			t.Fatal(err)
		}
		if len(links) != 1 {
			t.Fatalf("expected exactly 1 link, got %d", len(links))
		}
	}

	if err := st.UpsertGroupLink(g1.ID, g2.ID, true); err != nil {
		t.Fatal(err)
	}
	assertOneLink(t)

	if err := st.UpsertGroupLink(g2.ID, g1.ID, false); err != nil {
		t.Fatal(err)
	}
	assertOneLink(t)
	links, _ := st.ListGroupLinks()
	if links[0].Bidirectional {
		t.Fatal("expected unidirectional after replace")
	}
	if links[0].FromGroupID != g2.ID || links[0].ToGroupID != g1.ID {
		t.Fatalf("expected reversed uni link, got %+v", links[0])
	}

	if err := st.UpsertGroupLink(g1.ID, g2.ID, true); err != nil {
		t.Fatal(err)
	}
	assertOneLink(t)
	links, _ = st.ListGroupLinks()
	if !links[0].Bidirectional {
		t.Fatal("expected bidirectional after replace")
	}
}
