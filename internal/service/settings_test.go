package service

import (
	"errors"
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
)

func testSettingsApp(t *testing.T) *App {
	t.Helper()
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	if err := st.Setup(repo.SetupInput{
		Endpoint:         "example.com",
		Subnet:           "100.127.0.0/24",
		AdminUsername:    "admin",
		AdminPassword:    "password123",
		ListenPort:       8443,
		MTU:              1420,
		StatusInterval:   1,
		ServerPrivateKey: "priv",
		ServerPublicKey:  "pub",
	}); err != nil {
		t.Fatal(err)
	}
	return NewApp(st)
}

func TestChangeAdminPassword(t *testing.T) {
	app := testSettingsApp(t)

	if err := app.ChangeAdminPassword("admin", "password123", "newpassword123"); err != nil {
		t.Fatalf("ChangeAdminPassword failed: %v", err)
	}

	if _, err := app.VerifyAdminPassword("admin", "newpassword123"); err != nil {
		t.Fatalf("new password should verify: %v", err)
	}
}

func TestChangeAdminPasswordRejectsWrongCurrentPassword(t *testing.T) {
	app := testSettingsApp(t)

	err := app.ChangeAdminPassword("admin", "wrong", "newpassword123")
	if !errors.Is(err, ErrInvalidAdminPassword) {
		t.Fatalf("expected ErrInvalidAdminPassword, got %v", err)
	}
}

func TestVerifyAdminPassword(t *testing.T) {
	app := testSettingsApp(t)

	admin, err := app.VerifyAdminPassword("admin", "password123")
	if err != nil {
		t.Fatalf("VerifyAdminPassword failed: %v", err)
	}
	if admin.Username != "admin" {
		t.Fatalf("username = %q, want admin", admin.Username)
	}
}

func TestVerifyAdminPasswordRejectsWrongPassword(t *testing.T) {
	app := testSettingsApp(t)

	err := func() error {
		_, err := app.VerifyAdminPassword("admin", "wrong")
		return err
	}()
	if !errors.Is(err, ErrInvalidAdminPassword) {
		t.Fatalf("expected ErrInvalidAdminPassword, got %v", err)
	}
}
