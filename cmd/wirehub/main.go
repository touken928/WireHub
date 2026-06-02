package main

import (
	"log"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api"
	"github.com/touken928/wirehub/internal/auth"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/runtime"
	"github.com/touken928/wirehub/internal/static"
	"github.com/touken928/wirehub/internal/store"
)

func main() {
	cfg, err := config.ParseFlags()
	if err != nil {
		log.Fatalf("config: %v", err)
	}

	st, err := store.New(cfg)
	if err != nil {
		log.Fatalf("store: %v", err)
	}

	apiSvc := api.New(st)
	authSvc := auth.NewService(cfg.JWTSecret, st)

	gin.SetMode(gin.ReleaseMode)
	r := gin.New()
	r.Use(gin.Recovery(), gin.Logger())

	netRuntime := runtime.NewNetwork(cfg, st, apiSvc, r)
	apiSvc.SetNetworkController(netRuntime)

	api.RegisterRoutes(r, apiSvc, authSvc)
	if err := static.Mount(r); err != nil {
		log.Fatalf("static: %v", err)
	}

	configured, err := st.IsConfigured()
	if err != nil {
		log.Fatalf("setup check: %v", err)
	}
	if configured {
		if err := netRuntime.Start(); err != nil {
			log.Fatalf("network runtime: %v", err)
		}
	} else {
		log.Printf("WireHub setup required — open http://%s/setup", cfg.ListenAddr)
	}

	log.Printf("WireHub listening on %s", cfg.ListenAddr)
	if err := r.Run(cfg.ListenAddr); err != nil {
		log.Fatalf("http: %v", err)
	}
}
