package store

import (
	"reflect"
	"testing"
)

func TestParseUpstreamDNS(t *testing.T) {
	t.Run("empty uses defaults", func(t *testing.T) {
		got, err := ParseUpstreamDNS(nil)
		if err != nil {
			t.Fatal(err)
		}
		want := []string{"1.2.4.8", "1.1.1.1"}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("got %v, want %v", got, want)
		}
	})

	t.Run("newline separated", func(t *testing.T) {
		got, err := ParseUpstreamDNS([]string{"8.8.8.8\n8.8.4.4"})
		if err != nil {
			t.Fatal(err)
		}
		want := []string{"8.8.8.8", "8.8.4.4"}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("got %v, want %v", got, want)
		}
	})

	t.Run("invalid address", func(t *testing.T) {
		if _, err := ParseUpstreamDNS([]string{"not-an-ip"}); err == nil {
			t.Fatal("expected error")
		}
	})

	t.Run("dedupe", func(t *testing.T) {
		got, err := ParseUpstreamDNS([]string{"1.1.1.1", "1.1.1.1"})
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 || got[0] != "1.1.1.1" {
			t.Fatalf("got %v", got)
		}
	})
}

func TestSettingsClientDNS(t *testing.T) {
	s := &Settings{DNSIP: "100.127.0.1", UpstreamDNS: []string{"1.2.4.8", "1.1.1.1"}}
	got := s.ClientDNS()
	want := []string{"100.127.0.1", "1.2.4.8", "1.1.1.1"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
}
