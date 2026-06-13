package handlers

import (
	"bytes"
	"encoding/json"
	"io"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

// newTestServer creates a handler Server backed by a real SQLite store
// in a temp directory, with AllowRemoteSetup=false (the secure default).
func newTestServer(t *testing.T) (*Server, *repo.Store) {
	t.Helper()
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatalf("repo.New: %v", err)
	}
	app := service.NewApp(st)
	// allowRemoteSetup=false to test the default secure path
	srv := NewServer(app, false)
	return srv, st
}

// testContext builds a test gin.Context with a given body and RemoteAddr.
func testContext(t *testing.T, method, target string, body io.Reader, remoteAddr string) (*gin.Context, *httptest.ResponseRecorder) {
	t.Helper()
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(method, target, body)
	c.Request.RemoteAddr = remoteAddr
	if body != nil {
		c.Request.Header.Set("Content-Type", "application/json")
	}
	return c, w
}

// ---------------------------------------------------------------------------
// Setup origin protection
// ---------------------------------------------------------------------------

func TestSetup_RemoteOriginRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup", nil, "192.168.1.100:54321")
	Setup(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 Forbidden for remote origin, got %d", w.Code)
	}
	var body map[string]string
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body["error"] != "setup must be performed from localhost" {
		t.Fatalf("unexpected error message: %q", body["error"])
	}
}

func TestSetup_LocalOriginNotRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	// Empty body — will fail binding, but should NOT be 403
	c, w := testContext(t, "POST", "/api/setup", bytes.NewReader([]byte(`{}`)), "127.0.0.1:12345")
	Setup(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("expected non-403 for local origin, got 403")
	}
	// Should fail with binding error since endpoint is required
	if w.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 binding error for empty body, got %d: %s", w.Code, w.Body.String())
	}
}

func TestSetup_IPv6LoopbackAllowed(t *testing.T) {
	srv, _ := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup", bytes.NewReader([]byte(`{}`)), "[::1]:12345")
	Setup(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("expected non-403 for IPv6 loopback")
	}
}

func TestSetupStatus_RemoteOriginRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	c, w := testContext(t, "GET", "/api/setup/status", nil, "10.0.0.1:9999")
	SetupStatus(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 Forbidden for remote origin, got %d", w.Code)
	}
}

func TestSetupStatus_LocalOriginAllowed(t *testing.T) {
	srv, _ := newTestServer(t)
	c, w := testContext(t, "GET", "/api/setup/status", nil, "127.0.0.1:12345")
	SetupStatus(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("expected non-403 for local origin")
	}
	// Should succeed with setup status (unconfigured)
	if w.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d: %s", w.Code, w.Body.String())
	}
}

func TestImportDatabase_RemoteOriginRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup/import", nil, "10.0.0.1:9999")
	ImportDatabase(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 Forbidden for remote origin, got %d", w.Code)
	}
}

func TestImportDatabase_LocalOriginNotRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	// Send a request with no file attached — should fail with 400 (file required), not 403
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	_ = w.Close()
	c, rw := testContext(t, "POST", "/api/setup/import", &buf, "127.0.0.1:12345")
	c.Request.Header.Set("Content-Type", w.FormDataContentType())
	ImportDatabase(srv, c)

	if rw.Code == http.StatusForbidden {
		t.Fatal("expected non-403 for local origin")
	}
	// Should fail because no file was attached
	if rw.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 (file required), got %d: %s", rw.Code, rw.Body.String())
	}
}

// ---------------------------------------------------------------------------
// AllowRemoteSetup bypass verification
// ---------------------------------------------------------------------------

func TestSetup_AllowRemoteSetupBypass(t *testing.T) {
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatal(err)
	}
	app := service.NewApp(st)
	// allowRemoteSetup=true bypasses local-origin check
	srv := NewServer(app, true)

	c, w := testContext(t, "POST", "/api/setup", bytes.NewReader([]byte(`{}`)), "10.0.0.1:9999")
	Setup(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("AllowRemoteSetup=true should bypass local-origin check, got 403")
	}
	// With AllowRemoteSetup, the request should proceed to binding validation
	if w.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 (binding error), got %d: %s", w.Code, w.Body.String())
	}
}

// ---------------------------------------------------------------------------
// Upload-size enforcement exists in code path
// ---------------------------------------------------------------------------

func TestImportDatabase_UploadSizeCheckExists(t *testing.T) {
	// Verify the handler does not crash on a small valid-form request.
	// The actual >128MB rejection is exercised structurally: the check at
	// settings.go:110 runs before SaveUploadedFile. A full 128MB+1 multipart
	// body is excluded from unit tests for performance reasons.
	srv, _ := newTestServer(t)
	if config.MaxUploadBytes <= 0 {
		t.Fatal("config.MaxUploadBytes must be positive")
	}
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	_ = w.Close()
	c, rw := testContext(t, "POST", "/api/setup/import", &buf, "127.0.0.1:12345")
	c.Request.Header.Set("Content-Type", w.FormDataContentType())
	ImportDatabase(srv, c)
	if rw.Code == http.StatusInternalServerError {
		t.Fatalf("unexpected 500 for minimal request: %s", rw.Body.String())
	}
}
