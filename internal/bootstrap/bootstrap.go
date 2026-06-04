package bootstrap

import (
	"log"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	apihttp "github.com/touken928/wirehub/internal/api/http"
	"github.com/touken928/wirehub/internal/service"
	"github.com/touken928/wirehub/internal/static"
	"github.com/touken928/wirehub/internal/vpn/runtime"
)

// Run wires persistence, control plane, data plane, and HTTP serving.
func Run(cfg *config.RuntimeConfig) error {
	st, err := repo.New(cfg)
	if err != nil {
		return err
	}

	app := service.NewApp(st)
	apiSrv := apihttp.New(app, cfg.JWTSecret)

	gin.SetMode(gin.ReleaseMode)
	r := gin.New()
	r.Use(gin.Recovery(), gin.Logger())

	stack := runtime.NewStack(cfg, app, r)
	app.Hub.SetNetworkRuntime(stack)

	apihttp.RegisterRoutes(r, apiSrv)
	if err := static.Mount(r); err != nil {
		return err
	}

	configured, err := st.IsConfigured()
	if err != nil {
		return err
	}
	if configured {
		bundle, err := app.LoadSyncBundle()
		if err != nil {
			return err
		}
		if err := stack.Start(bundle); err != nil {
			return err
		}
	} else {
		log.Printf("WireHub setup required — open http://%s/setup", cfg.ListenAddr)
	}

	log.Printf("WireHub listening on %s", cfg.ListenAddr)
	return r.Run(cfg.ListenAddr)
}
