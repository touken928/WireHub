package config

import "testing"

func TestParseUpstreamDNS(t *testing.T) {
	t.Run("defaults when empty", func(t *testing.T) {
		got, err := ParseUpstreamDNS(nil)
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != len(DefaultUpstreamDNS) {
			t.Fatalf("got %v", got)
		}
	})

	t.Run("newline separated", func(t *testing.T) {
		got, err := ParseUpstreamDNS([]string{"8.8.8.8\n8.8.4.4"})
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 2 {
			t.Fatalf("got %v", got)
		}
	})

	t.Run("invalid", func(t *testing.T) {
		if _, err := ParseUpstreamDNS([]string{"not-an-ip"}); err == nil {
			t.Fatal("expected error")
		}
	})

	t.Run("dedupe", func(t *testing.T) {
		got, err := ParseUpstreamDNS([]string{"1.1.1.1", "1.1.1.1"})
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 {
			t.Fatalf("got %v", got)
		}
	})
}
