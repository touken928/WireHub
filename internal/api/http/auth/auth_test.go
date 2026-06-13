package auth

import (
	"path/filepath"
	"testing"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
)

// testStore creates a fresh in-memory store with one admin for testing.
func testStore(t *testing.T) *repo.Store {
	t.Helper()
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	hash, err := repo.HashPassword("testpass123")
	if err != nil {
		t.Fatal(err)
	}
	admin := &repo.Admin{
		Username:     "admin",
		PasswordHash: hash,
		TokenVersion: 0,
	}
	if err := st.DB().Create(admin).Error; err != nil {
		t.Fatal(err)
	}
	return st
}

func TestLoginAndParse_Success(t *testing.T) {
	st := testStore(t)
	svc := NewService("test-secret", st)

	token, err := svc.Login("admin", "testpass123")
	if err != nil {
		t.Fatalf("Login failed: %v", err)
	}
	if token == "" {
		t.Fatal("expected non-empty token")
	}

	claims, err := svc.ParseToken(token)
	if err != nil {
		t.Fatalf("ParseToken failed: %v", err)
	}
	if claims.Username != "admin" {
		t.Fatalf("expected username admin, got %s", claims.Username)
	}
	if claims.TokenVersion != 0 {
		t.Fatalf("expected token_version 0, got %d", claims.TokenVersion)
	}
}

func TestParseToken_InvalidCredentials(t *testing.T) {
	st := testStore(t)
	svc := NewService("test-secret", st)

	if _, err := svc.Login("admin", "wrongpass"); err == nil {
		t.Fatal("expected error for wrong password")
	}
	if _, err := svc.Login("nonexistent", "testpass123"); err == nil {
		t.Fatal("expected error for nonexistent user")
	}
}

func TestParseToken_WrongSecret(t *testing.T) {
	st := testStore(t)
	svc := NewService("test-secret", st)

	token, err := svc.Login("admin", "testpass123")
	if err != nil {
		t.Fatal(err)
	}

	svc2 := NewService("different-secret", st)
	if _, err := svc2.ParseToken(token); err == nil {
		t.Fatal("expected error when parsing token with wrong secret")
	}
}

func TestTokenInvalidatedAfterPasswordChange(t *testing.T) {
	st := testStore(t)
	svc := NewService("test-secret", st)

	// Get initial token
	oldToken, err := svc.Login("admin", "testpass123")
	if err != nil {
		t.Fatalf("Login failed: %v", err)
	}

	// Old token should work before password change
	if _, err := svc.ParseToken(oldToken); err != nil {
		t.Fatalf("old token should work before password change: %v", err)
	}

	// Change password — this increments TokenVersion
	admin, err := st.GetAdminByUsername("admin")
	if err != nil {
		t.Fatal(err)
	}
	if err := st.UpdateAdminPassword(admin.ID, "newstrongpass1"); err != nil {
		t.Fatalf("UpdateAdminPassword failed: %v", err)
	}

	// Old token should be rejected after password change
	if _, err := svc.ParseToken(oldToken); err == nil {
		t.Fatal("expected error: old token should be invalid after password change")
	}

	// New token should work
	newToken, err := svc.Login("admin", "newstrongpass1")
	if err != nil {
		t.Fatalf("Login with new password failed: %v", err)
	}
	if _, err := svc.ParseToken(newToken); err != nil {
		t.Fatalf("new token should work after password change: %v", err)
	}
}

func TestPasswordChangeEnforcesPolicy(t *testing.T) {
	st := testStore(t)

	admin, err := st.GetAdminByUsername("admin")
	if err != nil {
		t.Fatal(err)
	}

	// Attempt to set too-short password
	err = st.UpdateAdminPassword(admin.ID, "short")
	if err == nil {
		t.Fatal("expected error for too-short password")
	}
}

func TestParseToken_InvalidTokenString(t *testing.T) {
	st := testStore(t)
	svc := NewService("test-secret", st)

	if _, err := svc.ParseToken("not-a-valid-jwt"); err == nil {
		t.Fatal("expected error for invalid token string")
	}
	if _, err := svc.ParseToken(""); err == nil {
		t.Fatal("expected error for empty token string")
	}
}
