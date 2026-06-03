package server

import (
	"github.com/touken928/wirehub/internal/auth/limit"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
	"github.com/touken928/wirehub/internal/ws"
)

// Server is the HTTP delivery layer over the application Hub service.
type Server struct {
	*service.Hub
	statusHub    *ws.Hub
	loginLimiter *limit.Limiter
}

func New(st *repo.Store) *Server {
	s := &Server{
		Hub:          service.NewHub(st),
		loginLimiter: limit.DefaultLimiter(),
	}
	s.statusHub = ws.NewHub(s.buildStatusJSON)
	s.Hub.SetStatusPublisher(s.statusHub)
	return s
}
