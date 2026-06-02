package store

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
)

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

	if err := st.UpsertGroupLink(g1.ID, g2.ID); err != nil {
		t.Fatal(err)
	}
	if err := st.UpsertGroupLink(g2.ID, g1.ID); err != nil {
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

	exists, err := st.HasGroupLink(g2.ID, g1.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !exists {
		t.Fatal("expected link to exist")
	}
}
