package handlers

import (
	"crypto/rand"
	"encoding/hex"
	"log"
	"sync"

	"github.com/touken928/wirehub/internal/api/http/httputil"
	"github.com/touken928/wirehub/internal/api/ws"
	"github.com/touken928/wirehub/internal/service"
)

// Server holds HTTP handler dependencies (no Gin types).
type Server struct {
	App          *service.App
	StatusWS     *ws.Hub
	loginLimiter *httputil.LoginRateLimiter
	SetupURLHost string // host:port shown in setup URL hints

	// Protected by mu for concurrent-safe Reset + setup-endpoint reads.
	mu         sync.RWMutex
	setupToken string // first-run setup token; empty when already configured
}

// GetSetupToken returns the current setup token under the read lock.
func (s *Server) GetSetupToken() string {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.setupToken
}

// setSetupToken writes the token under the write lock.
func (s *Server) setSetupToken(tok string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.setupToken = tok
}

// LoginLimiter returns the login rate limiter.
func (s *Server) LoginLimiter() *httputil.LoginRateLimiter {
	return s.loginLimiter
}

// RegenerateSetupToken creates a new random setup token, stores it on the server,
// and logs it with operator hints. Used after first-run startup and after a hub reset.
func (s *Server) RegenerateSetupToken() string {
	b := make([]byte, 32)
	if _, err := rand.Read(b); err != nil {
		log.Fatalf("setup token generation: %v", err)
	}
	token := hex.EncodeToString(b)
	s.setSetupToken(token)
	log.Println("══════════════════════════════════════════════════════")
	log.Printf("  New setup token: %s", token)
	log.Printf("  Open http://%s/setup?setup_token=%s", s.SetupURLHost, token)
	log.Printf("  Keep the token in the setup URL until configuration completes")
	log.Println("══════════════════════════════════════════════════════")
	return token
}

// NewServer constructs handler dependencies and wires status WebSocket publishing.
func NewServer(app *service.App, setupToken string) *Server {
	s := &Server{
		App:          app,
		loginLimiter: httputil.DefaultLoginRateLimiter(),
	}
	s.setSetupToken(setupToken)
	s.StatusWS = ws.NewHub(app.Status.BuildJSON)
	app.Status.SetNotifier(s.StatusWS.Publish)
	return s
}
