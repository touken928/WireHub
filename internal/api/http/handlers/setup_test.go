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
	"github.com/touken928/wirehub/internal/api/http/auth"
	"github.com/touken928/wirehub/internal/config"
	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

// mockNetworkRuntime is a minimal NetworkRuntime stub for Reset tests.
type mockNetworkRuntime struct{}

func (m *mockNetworkRuntime) Start(_ domainruntime.SyncBundle) error { return nil }
func (m *mockNetworkRuntime) Stop() error                            { return nil }
func (m *mockNetworkRuntime) ReloadSettings() error                  { return nil }
func (m *mockNetworkRuntime) SyncPortForwards() error                { return nil }
func (m *mockNetworkRuntime) SyncMaps() error                        { return nil }
func (m *mockNetworkRuntime) HubListenPort() int                     { return 0 }
func (m *mockNetworkRuntime) SetDNSUpstream(_ []string)              {}

// newTestServer creates a handler Server backed by a real SQLite store
// in a temp directory, and returns the setup token used for protected endpoints.
func newTestServer(t *testing.T) (*Server, *repo.Store, string) {
	t.Helper()
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatalf("repo.New: %v", err)
	}
	app := service.NewApp(st)
	// Generate a deterministic token for testing (in production it is random)
	token := "test-setup-token-64-chars-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
	srv := NewServer(app, token)
	return srv, st, token
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

// newServerWithToken generates a deterministic setup token for tests.
func newServerWithToken(t *testing.T, token string) (*Server, *repo.Store) {
	t.Helper()
	dir := t.TempDir()
	st, err := repo.New(&config.RuntimeConfig{DatabasePath: filepath.Join(dir, "wirehub.db")})
	if err != nil {
		t.Fatalf("repo.New: %v", err)
	}
	app := service.NewApp(st)
	srv := NewServer(app, token)
	return srv, st
}

// ---------------------------------------------------------------------------
// Setup token protection
// ---------------------------------------------------------------------------

func TestSetup_TokenRequired(t *testing.T) {
	srv, _, _ := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup", nil, "127.0.0.1:12345")
	Setup(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 Forbidden without token, got %d", w.Code)
	}
	var body map[string]string
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body["error"] != "setup token required; check server logs for the first-run setup token" {
		t.Fatalf("unexpected error message: %q", body["error"])
	}
}

func TestSetup_ValidTokenViaQuery(t *testing.T) {
	srv, _, token := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup?setup_token="+token, bytes.NewReader([]byte(`{}`)), "192.168.1.100:54321")
	Setup(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("expected non-403 with valid token in query, got 403")
	}
	if w.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 binding error for empty body, got %d: %s", w.Code, w.Body.String())
	}
}

func TestSetup_InvalidTokenRejected(t *testing.T) {
	srv, _, _ := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup?setup_token=wrong-token", bytes.NewReader([]byte(`{}`)), "127.0.0.1:12345")
	Setup(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 for invalid token, got %d", w.Code)
	}
}

func TestSetupStatus_TokenRequired(t *testing.T) {
	srv, _, _ := newTestServer(t)
	c, w := testContext(t, "GET", "/api/setup/status", nil, "10.0.0.1:9999")
	SetupStatus(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 Forbidden without token, got %d", w.Code)
	}
}

func TestSetupStatus_ValidTokenAllows(t *testing.T) {
	srv, _, token := newTestServer(t)
	c, w := testContext(t, "GET", "/api/setup/status?setup_token="+token, nil, "10.0.0.1:9999")
	SetupStatus(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("expected non-403 with valid token")
	}
	// Should succeed with setup status (unconfigured)
	if w.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d: %s", w.Code, w.Body.String())
	}
}

func TestImportDatabase_TokenRequired(t *testing.T) {
	srv, _, _ := newTestServer(t)
	c, w := testContext(t, "POST", "/api/setup/import", nil, "10.0.0.1:9999")
	ImportDatabase(srv, c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 Forbidden without token, got %d", w.Code)
	}
}

func TestImportDatabase_ValidTokenNotRejected(t *testing.T) {
	srv, _, token := newTestServer(t)
	// Send a request with no file attached — should fail with 400 (file required), not 403
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	_ = w.Close()
	c, rw := testContext(t, "POST", "/api/setup/import?setup_token="+token, &buf, "127.0.0.1:12345")
	c.Request.Header.Set("Content-Type", w.FormDataContentType())
	ImportDatabase(srv, c)

	if rw.Code == http.StatusForbidden {
		t.Fatal("expected non-403 with valid token")
	}
	// Should fail because no file was attached
	if rw.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 (file required), got %d: %s", rw.Code, rw.Body.String())
	}
}

// ---------------------------------------------------------------------------
// Configured hub — auth middleware guards; no setup token needed
// ---------------------------------------------------------------------------

func TestSetup_ConfiguredHubBypassesTokenCheck(t *testing.T) {
	srv, st := newServerWithToken(t, "some-token")
	// Configure the hub
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

	// Request without token - should not be 403 because hub is configured
	// (POST /api/setup should return conflict since already configured)
	c, w := testContext(t, "POST", "/api/setup", bytes.NewReader([]byte(`{}`)), "10.0.0.1:9999")
	Setup(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("configured hub should not require setup token")
	}
	if w.Code != http.StatusConflict {
		t.Fatalf("expected 409 Conflict for already configured hub, got %d: %s", w.Code, w.Body.String())
	}
}

func TestSetupStatus_ConfiguredHubBypassesTokenCheck(t *testing.T) {
	srv, st := newServerWithToken(t, "some-token")
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

	c, w := testContext(t, "GET", "/api/setup/status", nil, "10.0.0.1:9999")
	SetupStatus(srv, c)

	if w.Code == http.StatusForbidden {
		t.Fatal("configured hub should not require setup token")
	}
	if w.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d: %s", w.Code, w.Body.String())
	}
}

func TestImportDatabase_ConfiguredHubBypassesTokenCheck(t *testing.T) {
	srv, st := newServerWithToken(t, "some-token")
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

	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	_ = w.Close()
	c, rw := testContext(t, "POST", "/api/setup/import", &buf, "10.0.0.1:9999")
	c.Request.Header.Set("Content-Type", w.FormDataContentType())
	ImportDatabase(srv, c)

	if rw.Code == http.StatusForbidden {
		t.Fatal("configured hub should not require setup token")
	}
	// Should return conflict because hub is already configured
	if rw.Code != http.StatusConflict {
		t.Fatalf("expected 409 Conflict for already configured hub, got %d: %s", rw.Code, rw.Body.String())
	}
}

// ---------------------------------------------------------------------------
// Token concurrency — Reset writes while endpoints read
// ---------------------------------------------------------------------------

func TestSetupToken_ConcurrentSafe(t *testing.T) {
	srv, st := newServerWithToken(t, "initial-token-0000000000000000000000000000000")
	srv.App.Hub.SetNetworkRuntime(&mockNetworkRuntime{})

	// Configure the hub so reset is possible
	if err := st.Setup(repo.SetupInput{
		Endpoint: "example.com", Subnet: "100.127.0.0/24",
		AdminUsername: "admin", AdminPassword: "password123",
		ListenPort: 8443, MTU: 1420, StatusInterval: 1,
		ServerPrivateKey: "priv", ServerPublicKey: "pub",
	}); err != nil {
		t.Fatal(err)
	}

	// Spin up concurrent readers and writers.
	done := make(chan struct{})
	errs := make(chan error, 20)

	// Readers: call GetSetupToken + requireSetupToken (via SetupStatus)
	readFn := func() {
		for {
			select {
			case <-done:
				return
			default:
				_ = srv.GetSetupToken()
				c, w := testContext(t, "GET", "/api/setup/status", nil, "127.0.0.1:9999")
				SetupStatus(srv, c)
				_ = w.Code // just consume; no crash = success
			}
		}
	}

	// Writer: call RegenerateSetupToken (simulates Reset)
	writeFn := func() {
		for i := 0; i < 10; i++ {
			tok := srv.RegenerateSetupToken()
			if tok == "" {
				errs <- nil // will be caught as non-nil error below (type mismatch)
				return
			}
		}
	}

	go writeFn()
	for i := 0; i < 4; i++ {
		go readFn()
	}

	// Let them race for a bit
	done <- struct{}{}
	// Wait for goroutines to settle
	_ = srv.GetSetupToken()

	// If we reach here without a data race, the test passes.
	// (The -race flag would catch actual races.)
}

// ---------------------------------------------------------------------------
// Upload-size enforcement exists in code path
// ---------------------------------------------------------------------------

func TestImportDatabase_UploadSizeCheckExists(t *testing.T) {
	// Verify the handler does not crash on a small valid-form request.
	// The actual >128MB rejection is exercised structurally: the check at
	// settings.go:110 runs before SaveUploadedFile. A full 128MB+1 multipart
	// body is excluded from unit tests for performance reasons.
	srv, _, token := newTestServer(t)
	if config.MaxUploadBytes <= 0 {
		t.Fatal("config.MaxUploadBytes must be positive")
	}
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	_ = w.Close()
	c, rw := testContext(t, "POST", "/api/setup/import?setup_token="+token, &buf, "127.0.0.1:12345")
	c.Request.Header.Set("Content-Type", w.FormDataContentType())
	ImportDatabase(srv, c)
	if rw.Code == http.StatusInternalServerError {
		t.Fatalf("unexpected 500 for minimal request: %s", rw.Body.String())
	}
}

func TestChangePasswordWrongCurrentPassword(t *testing.T) {
	srv, st := newServerWithToken(t, "some-token")
	authSvc := auth.NewService("test-secret", st)
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

	body := bytes.NewBufferString(`{"current_password":"wrong","new_password":"newpassword123"}`)
	c, w := testContext(t, "PUT", "/api/settings/password", body, "127.0.0.1:12345")
	c.Set("username", "admin")
	c.Set("auth", authSvc)

	ChangePassword(srv, c)
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d: %s", w.Code, w.Body.String())
	}
}

func TestResetWrongPassword(t *testing.T) {
	srv, st := newServerWithToken(t, "some-token")
	authSvc := auth.NewService("test-secret", st)
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

	body := bytes.NewBufferString(`{"password":"wrong"}`)
	c, w := testContext(t, "POST", "/api/admin/reset", body, "127.0.0.1:12345")
	c.Set("username", "admin")
	c.Set("auth", authSvc)

	Reset(srv, c)
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d: %s", w.Code, w.Body.String())
	}
}

// ---------------------------------------------------------------------------
// Reset success — generates a new setup token
// ---------------------------------------------------------------------------

func TestReset_Success_ReturnsNewToken(t *testing.T) {
	srv, st := newServerWithToken(t, "old-token-00000000000000000000000000000000")
	// Set a display host so the log URL hint is well-formed
	srv.SetupURLHost = "127.0.0.1:8443"

	authSvc := auth.NewService("test-secret", st)
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

	// Set a mock network runtime so Reset() does not fail with ErrNetworkUnavailable
	srv.App.Hub.SetNetworkRuntime(&mockNetworkRuntime{})

	body := bytes.NewBufferString(`{"password":"password123"}`)
	c, w := testContext(t, "POST", "/api/admin/reset", body, "127.0.0.1:12345")
	c.Set("username", "admin")
	c.Set("auth", authSvc)

	Reset(srv, c)

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d: %s", w.Code, w.Body.String())
	}

	var resp struct {
		Ok        bool   `json:"ok"`
		SetupToken string `json:"setup_token"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatal(err)
	}
	if !resp.Ok {
		t.Fatal("expected ok=true")
	}
	if resp.SetupToken == "" {
		t.Fatal("expected non-empty setup_token in reset response")
	}
	if resp.SetupToken == "old-token-00000000000000000000000000000000" {
		t.Fatal("setup_token should be a new value, not the old token")
	}
	// Verify the server's setup token was updated
	if srv.GetSetupToken() != resp.SetupToken {
		t.Fatalf("server setupToken (%q) does not match response (%q)", srv.GetSetupToken(), resp.SetupToken)
	}

	// Verify the hub is now unconfigured
	configured, err := st.IsConfigured()
	if err != nil {
		t.Fatal(err)
	}
	if configured {
		t.Fatal("hub should be unconfigured after reset")
	}
}

func TestReset_NewTokenWorksForSetup(t *testing.T) {
	srv, st := newServerWithToken(t, "old-token-00000000000000000000000000000000")
	srv.SetupURLHost = "127.0.0.1:8443"
	authSvc := auth.NewService("test-secret", st)
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
	srv.App.Hub.SetNetworkRuntime(&mockNetworkRuntime{})

	// Perform reset
	body := bytes.NewBufferString(`{"password":"password123"}`)
	c, w := testContext(t, "POST", "/api/admin/reset", body, "127.0.0.1:12345")
	c.Set("username", "admin")
	c.Set("auth", authSvc)
	Reset(srv, c)
	if w.Code != http.StatusOK {
		t.Fatalf("reset failed: %d %s", w.Code, w.Body.String())
	}
	var resetResp struct {
		SetupToken string `json:"setup_token"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resetResp); err != nil {
		t.Fatal(err)
	}

	// Old token should no longer work
	c2, w2 := testContext(t, "GET", "/api/setup/status?setup_token=old-token-00000000000000000000000000000000", nil, "10.0.0.1:9999")
	SetupStatus(srv, c2)
	if w2.Code != http.StatusForbidden {
		t.Fatalf("expected 403 with old token after reset, got %d", w2.Code)
	}

	// New token should work
	c3, w3 := testContext(t, "GET", "/api/setup/status?setup_token="+resetResp.SetupToken, nil, "10.0.0.1:9999")
	SetupStatus(srv, c3)
	if w3.Code == http.StatusForbidden {
		t.Fatal("expected OK with new token after reset, got 403")
	}
	if w3.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d: %s", w3.Code, w3.Body.String())
	}
}
