package apihttp

import (
	"github.com/touken928/wirehub/internal/api/http/auth"
	"github.com/touken928/wirehub/internal/api/http/handlers"
	"github.com/touken928/wirehub/internal/service"
)

// Server is the HTTP delivery layer over service.App.
type Server struct {
	*handlers.Server
	Auth *auth.Service
}

// New constructs the API server and wires status WebSocket publishing.
// setupToken is the first-run token (empty when hub is already configured).
// setupURLHost is the host:port shown in setup URL hints.
func New(app *service.App, jwtSecret string, setupToken string, setupURLHost string) *Server {
	s := handlers.NewServer(app, setupToken)
	s.SetupURLHost = setupURLHost
	return &Server{
		Server: s,
		Auth:   auth.NewService(jwtSecret, app.Store()),
	}
}
