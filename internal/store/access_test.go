package store

import "testing"

func TestResolveExcludeRules(t *testing.T) {
	peers := []Peer{
		{Name: "alice", WGIP: "100.127.0.2"},
		{Name: "bob", WGIP: "100.127.0.3"},
		{Name: "server-01", WGIP: "100.127.0.4"},
	}
	self := Peer{Name: "guest", WGIP: "100.127.0.5"}

	t.Run("empty means unrestricted", func(t *testing.T) {
		ips, err := ResolveExcludeRules(peers, self, nil)
		if err != nil {
			t.Fatal(err)
		}
		if len(ips) != 0 {
			t.Fatalf("got %v", ips)
		}
	})

	t.Run("exclude hostname", func(t *testing.T) {
		ips, err := ResolveExcludeRules(peers, self, []string{"alice"})
		if err != nil {
			t.Fatal(err)
		}
		if len(ips) != 1 || ips[0] != "100.127.0.2" {
			t.Fatalf("got %v", ips)
		}
	})

	t.Run("domain suffix rejected", func(t *testing.T) {
		if _, err := ResolveExcludeRules(peers, self, []string{"alice.local"}); err == nil {
			t.Fatal("expected error")
		}
	})

	t.Run("wildcard prefix", func(t *testing.T) {
		ips, err := ResolveExcludeRules(peers, self, []string{"server-*"})
		if err != nil {
			t.Fatal(err)
		}
		if len(ips) != 1 || ips[0] != "100.127.0.4" {
			t.Fatalf("got %v", ips)
		}
	})

	t.Run("negation re-allows", func(t *testing.T) {
		lines := []string{"*", "!bob"}
		ips, err := ResolveExcludeRules(peers, self, lines)
		if err != nil {
			t.Fatal(err)
		}
		if len(ips) != 2 {
			t.Fatalf("got %v, want alice and server-01 blocked", ips)
		}
	})

	t.Run("comment ignored", func(t *testing.T) {
		ips, err := ResolveExcludeRules(peers, self, []string{"# comment", "alice"})
		if err != nil {
			t.Fatal(err)
		}
		if len(ips) != 1 || ips[0] != "100.127.0.2" {
			t.Fatalf("got %v", ips)
		}
	})

	t.Run("self reference denied", func(t *testing.T) {
		if _, err := ResolveExcludeRules(peers, self, []string{"guest"}); err == nil {
			t.Fatal("expected error")
		}
	})
}

func TestParseExcludeLines(t *testing.T) {
	got := ParseExcludeLines([]string{"# all peers\nalice", "bob", "", "  "})
	if len(got) != 2 || got[0] != "alice" || got[1] != "bob" {
		t.Fatalf("got %v", got)
	}
}
