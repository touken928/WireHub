package config

import (
	"os"
	"testing"
)

func TestFirstHostIP(t *testing.T) {
	got, err := FirstHostIP("100.127.0.0/24")
	if err != nil {
		t.Fatal(err)
	}
	if got != "100.127.0.1" {
		t.Fatalf("got %s, want 100.127.0.1", got)
	}
}

func TestParseFlagsDefaults(t *testing.T) {
	dir := t.TempDir()
	oldArgs := os.Args
	defer func() { os.Args = oldArgs }()
	os.Args = []string{"wirehub", "-data-dir", dir}

	cfg, err := ParseFlags()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Port != DefaultWebPort {
		t.Fatalf("port = %d, want %d", cfg.Port, DefaultWebPort)
	}
	if cfg.Bind != DefaultBind {
		t.Fatalf("bind = %q, want %q", cfg.Bind, DefaultBind)
	}
	if cfg.ListenAddr != "0.0.0.0:8443" {
		t.Fatalf("listen addr = %q", cfg.ListenAddr)
	}
	if cfg.JWTSecret == "" {
		t.Fatal("expected auto-generated jwt secret")
	}
	if cfg.DatabasePath == "" {
		t.Fatal("expected database path")
	}
}
