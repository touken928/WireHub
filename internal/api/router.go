package api

import (
	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/auth"
)

func RegisterRoutes(r *gin.Engine, svc *Server, authSvc *auth.Service) {
	r.Use(func(c *gin.Context) {
		c.Set("auth", authSvc)
		c.Next()
	})

	api := r.Group("/api")
	{
		api.GET("/setup/status", svc.handleSetupStatus)
		api.POST("/setup", svc.handleSetup)
		api.POST("/setup/import", svc.handleImportDatabase)
		api.POST("/auth/login", svc.handleLogin)

		protected := api.Group("")
		protected.Use(auth.Middleware(authSvc))
		{
			protected.POST("/admin/reset", svc.handleReset)
			protected.GET("/status", svc.handleStatus)
			protected.GET("/settings", svc.handleGetSettings)
			protected.PUT("/settings", svc.handleUpdateSettings)
			protected.PUT("/settings/password", svc.handleChangePassword)
			protected.GET("/settings/export", svc.handleExportDatabase)
			protected.GET("/groups", svc.handleListGroups)
			protected.POST("/groups", svc.handleCreateGroup)
			protected.PUT("/groups/:id", svc.handleUpdateGroup)
			protected.DELETE("/groups/:id", svc.handleDeleteGroup)
			protected.GET("/groups/graph", svc.handleGroupGraph)
			protected.POST("/groups/links", svc.handleCreateGroupLink)
			protected.DELETE("/groups/links", svc.handleDeleteGroupLink)
			protected.PUT("/groups/layout", svc.handleUpdateGroupLayout)
			protected.GET("/peers", svc.handleListPeers)
			protected.POST("/peers", svc.handleCreatePeer)
			protected.PUT("/peers/:id", svc.handleUpdatePeer)
			protected.DELETE("/peers/:id", svc.handleDeletePeer)
			protected.POST("/peers/:id/toggle", svc.handleTogglePeer)
			protected.GET("/peers/:id/config", svc.handlePeerConfig)
		}
	}
}
