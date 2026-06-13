package bootstrap

import (
	"context"
	"io"
	"log"
	"net/http"
	"os"
	"os/signal"
	"regexp"
	"syscall"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	apihttp "github.com/touken928/wirehub/internal/api/http"
	"github.com/touken928/wirehub/internal/service"
	"github.com/touken928/wirehub/internal/static"
	"github.com/touken928/wirehub/internal/vpn/runtime"
)

// tokenRE matches token= in query strings so they can be redacted from logs.
var tokenRE = regexp.MustCompile(`token=[^&\s]+`)

// redactingWriter wraps an io.Writer and redacts sensitive query parameters.
type redactingWriter struct {
	inner io.Writer
}

func (w *redactingWriter) Write(p []byte) (int, error) {
	s := tokenRE.ReplaceAllString(string(p), "token=REDACTED")
	return w.inner.Write([]byte(s))
}

// Run wires persistence, control plane, data plane, and HTTP serving.
// Blocks until SIGINT/SIGTERM, then performs graceful shutdown.
func Run(cfg *config.RuntimeConfig) error {
	st, err := repo.New(cfg)
	if err != nil {
		return err
	}

	app := service.NewApp(st)
	apiSrv := apihttp.New(app, cfg.JWTSecret, cfg)

	gin.SetMode(gin.ReleaseMode)
	r := gin.New()
	r.Use(gin.Recovery())
	r.Use(gin.LoggerWithWriter(&redactingWriter{inner: os.Stdout}))

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

	// Start background cleanup for the login rate limiter (idle entry eviction).
	if lim := apiSrv.LoginLimiter(); lim != nil {
		lim.StartCleanupLoop()
	}

	srv := &http.Server{
		Addr:              cfg.ListenAddr,
		Handler:           r,
		ReadHeaderTimeout: 10 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
	}

	// Start server in background.
	go func() {
		log.Printf("WireHub listening on %s", cfg.ListenAddr)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("listen: %v", err)
		}
	}()

	// Wait for interrupt signal for graceful shutdown.
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit
	log.Println("Shutting down server...")

	// Stop the login rate limiter cleanup goroutine before server shutdown.
	if lim := apiSrv.LoginLimiter(); lim != nil {
		lim.StopCleanupLoop()
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return srv.Shutdown(ctx)
}
