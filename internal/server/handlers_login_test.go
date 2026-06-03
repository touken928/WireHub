package server

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/auth"
	"github.com/touken928/wirehub/internal/auth/limit"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

func TestHandleLogin_RateLimit(t *testing.T) {
	gin.SetMode(gin.TestMode)

	dir := t.TempDir()
	cfg := &config.RuntimeConfig{
		DatabasePath: filepath.Join(dir, "wirehub.db"),
		JWTSecret:    "test-jwt-secret",
	}
	st, err := repo.New(cfg)
	if err != nil {
		t.Fatalf("repo.New: %v", err)
	}
	if err := st.Setup(repo.SetupInput{
		Endpoint:         "hub.example.com",
		Subnet:           config.DefaultSubnet,
		AdminUsername:    "admin",
		AdminPassword:    "password123",
		ListenPort:       config.DefaultEndpointPort,
		ServerPrivateKey: "dummy-priv",
		ServerPublicKey:  "dummy-pub",
	}); err != nil {
		t.Fatalf("Setup: %v", err)
	}

	authSvc := auth.NewService(cfg.JWTSecret, st)
	svc := &Server{
		Hub: service.NewHub(st),
		loginLimiter: limit.New(limit.Config{
			Capacity:     3,
			RefillPeriod: time.Minute,
		}),
	}

	r := gin.New()
	r.Use(func(c *gin.Context) {
		c.Set("auth", authSvc)
		c.Next()
	})
	r.POST("/api/auth/login", svc.handleLogin)

	body, _ := json.Marshal(loginRequest{Username: "admin", Password: "wrong"})

	for i := 0; i < 3; i++ {
		req := httptest.NewRequest(http.MethodPost, "/api/auth/login", bytes.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.RemoteAddr = "203.0.113.50:54321"
		w := httptest.NewRecorder()
		r.ServeHTTP(w, req)
		if w.Code != http.StatusUnauthorized {
			t.Fatalf("attempt %d: status = %d, want 401", i+1, w.Code)
		}
	}

	req := httptest.NewRequest(http.MethodPost, "/api/auth/login", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.RemoteAddr = "203.0.113.50:54321"
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)
	if w.Code != http.StatusTooManyRequests {
		t.Fatalf("status = %d, want 429", w.Code)
	}
	if got := w.Header().Get("Retry-After"); got == "" {
		t.Fatal("missing Retry-After header")
	}

	okBody, _ := json.Marshal(loginRequest{Username: "admin", Password: "password123"})
	req = httptest.NewRequest(http.MethodPost, "/api/auth/login", bytes.NewReader(okBody))
	req.Header.Set("Content-Type", "application/json")
	req.RemoteAddr = "203.0.113.51:54321"
	w = httptest.NewRecorder()
	r.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("other ip login status = %d, want 200", w.Code)
	}
}

func TestHandleLogin_SuccessClearsFailures(t *testing.T) {
	gin.SetMode(gin.TestMode)

	dir := t.TempDir()
	cfg := &config.RuntimeConfig{
		DatabasePath: filepath.Join(dir, "wirehub.db"),
		JWTSecret:    "test-jwt-secret",
	}
	st, err := repo.New(cfg)
	if err != nil {
		t.Fatalf("repo.New: %v", err)
	}
	if err := st.Setup(repo.SetupInput{
		Endpoint:         "hub.example.com",
		Subnet:           config.DefaultSubnet,
		AdminUsername:    "admin",
		AdminPassword:    "password123",
		ListenPort:       config.DefaultEndpointPort,
		ServerPrivateKey: "dummy-priv",
		ServerPublicKey:  "dummy-pub",
	}); err != nil {
		t.Fatalf("Setup: %v", err)
	}

	authSvc := auth.NewService(cfg.JWTSecret, st)
	svc := &Server{
		Hub: service.NewHub(st),
		loginLimiter: limit.New(limit.Config{
			Capacity:     3,
			RefillPeriod: time.Minute,
		}),
	}

	r := gin.New()
	r.Use(func(c *gin.Context) {
		c.Set("auth", authSvc)
		c.Next()
	})
	r.POST("/api/auth/login", svc.handleLogin)

	bad, _ := json.Marshal(loginRequest{Username: "admin", Password: "wrong"})
	good, _ := json.Marshal(loginRequest{Username: "admin", Password: "password123"})
	ip := "203.0.113.60:54321"

	for i := 0; i < 2; i++ {
		req := httptest.NewRequest(http.MethodPost, "/api/auth/login", bytes.NewReader(bad))
		req.Header.Set("Content-Type", "application/json")
		req.RemoteAddr = ip
		w := httptest.NewRecorder()
		r.ServeHTTP(w, req)
		if w.Code != http.StatusUnauthorized {
			t.Fatalf("bad attempt %d: status = %d", i+1, w.Code)
		}
	}

	req := httptest.NewRequest(http.MethodPost, "/api/auth/login", bytes.NewReader(good))
	req.Header.Set("Content-Type", "application/json")
	req.RemoteAddr = ip
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("good login status = %d, want 200", w.Code)
	}

	req = httptest.NewRequest(http.MethodPost, "/api/auth/login", bytes.NewReader(bad))
	req.Header.Set("Content-Type", "application/json")
	req.RemoteAddr = ip
	w = httptest.NewRecorder()
	r.ServeHTTP(w, req)
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("after success, bad login status = %d, want 401 (not locked)", w.Code)
	}
}
