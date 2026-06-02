package server

import (
	"github.com/touken928/wirehub/internal/service"
	"github.com/touken928/wirehub/internal/repo"
)

// Server is the HTTP delivery layer over the application Hub service.
type Server struct {
	*service.Hub
}

func New(st *repo.Store) *Server {
	return &Server{Hub: service.NewHub(st)}
}
