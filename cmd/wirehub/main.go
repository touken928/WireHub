package main

import (
	"log"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/auth"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/server"
	"github.com/touken928/wirehub/internal/static"
	"github.com/touken928/wirehub/internal/vpn"
)

func main() {
	cfg, err := config.ParseFlags()
	if err != nil {
		log.Fatalf("config: %v", err)
	}

	st, err := repo.New(cfg)
	if err != nil {
		log.Fatalf("repo: %v", err)
	}

	svc := server.New(st)
	authSvc := auth.NewService(cfg.JWTSecret, st)

	gin.SetMode(gin.ReleaseMode)
	r := gin.New()
	r.Use(gin.Recovery(), gin.Logger())

	stack := vpn.NewStack(cfg, st, svc.Hub, r)
	svc.SetNetworkRuntime(stack)

	server.RegisterRoutes(r, svc, authSvc)
	if err := static.Mount(r); err != nil {
		log.Fatalf("static: %v", err)
	}

	configured, err := st.IsConfigured()
	if err != nil {
		log.Fatalf("setup check: %v", err)
	}
	if configured {
		if err := stack.Start(); err != nil {
			log.Fatalf("vpn stack: %v", err)
		}
	} else {
		log.Printf("WireHub setup required — open http://%s/setup", cfg.ListenAddr)
	}

	log.Printf("WireHub listening on %s", cfg.ListenAddr)
	if err := r.Run(cfg.ListenAddr); err != nil {
		log.Fatalf("http: %v", err)
	}
}
