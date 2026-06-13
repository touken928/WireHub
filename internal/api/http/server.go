package apihttp

import (
	"github.com/touken928/wirehub/internal/api/http/auth"
	"github.com/touken928/wirehub/internal/api/http/handlers"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/service"
)

// Server is the HTTP delivery layer over service.App.
type Server struct {
	*handlers.Server
	Auth *auth.Service
}

// New constructs the API server and wires status WebSocket publishing.
func New(app *service.App, jwtSecret string, cfg *config.RuntimeConfig) *Server {
	return &Server{
		Server: handlers.NewServer(app, cfg.AllowRemoteSetup),
		Auth:   auth.NewService(jwtSecret, app.Store),
	}
}
