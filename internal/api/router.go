package api

import (
	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/auth"
)

func RegisterRoutes(r *gin.Engine, svc *Server, authSvc *auth.Service, listenPort int) {
	r.Use(func(c *gin.Context) {
		c.Set("auth", authSvc)
		c.Set("listen_port", listenPort)
		c.Next()
	})

	api := r.Group("/api")
	{
		api.GET("/setup/status", svc.handleSetupStatus)
		api.POST("/setup", svc.handleSetup)
		api.POST("/auth/login", svc.handleLogin)

		protected := api.Group("")
		protected.Use(auth.Middleware(authSvc))
		{
			protected.POST("/admin/reset", svc.handleReset)
			protected.GET("/status", svc.handleStatus)
			protected.GET("/peers", svc.handleListPeers)
			protected.POST("/peers", svc.handleCreatePeer)
			protected.PUT("/peers/:id", svc.handleUpdatePeer)
			protected.DELETE("/peers/:id", svc.handleDeletePeer)
			protected.POST("/peers/:id/toggle", svc.handleTogglePeer)
			protected.GET("/peers/:id/config", svc.handlePeerConfig)
		}
	}
}
