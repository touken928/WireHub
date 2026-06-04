package handlers

import (
	"github.com/touken928/wirehub/internal/api/http/httputil"
	"github.com/touken928/wirehub/internal/api/ws"
	"github.com/touken928/wirehub/internal/service"
)

// Server holds HTTP handler dependencies (no Gin types).
type Server struct {
	App          *service.App
	StatusWS     *ws.Hub
	loginLimiter *httputil.LoginRateLimiter
}

// LoginLimiter returns the login rate limiter.
func (s *Server) LoginLimiter() *httputil.LoginRateLimiter {
	return s.loginLimiter
}

// NewServer constructs handler dependencies and wires status WebSocket publishing.
func NewServer(app *service.App) *Server {
	s := &Server{
		App:          app,
		loginLimiter: httputil.DefaultLoginRateLimiter(),
	}
	s.StatusWS = ws.NewHub(app.Status.BuildJSON)
	app.Status.SetNotifier(s.StatusWS.Publish)
	return s
}
