package apihttp

import (
	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/handlers"
	"github.com/touken928/wirehub/internal/api/http/auth"
)

// RegisterRoutes mounts the REST API and WebSocket endpoints.
func RegisterRoutes(r *gin.Engine, svc *Server) {
	r.Use(func(c *gin.Context) {
		c.Set("auth", svc.Auth)
		c.Next()
	})

	api := r.Group("/api")
	{
		api.GET("/setup/status", func(c *gin.Context) { handlers.SetupStatus(svc.Server, c) })
		api.POST("/setup", func(c *gin.Context) { handlers.Setup(svc.Server, c) })
		api.POST("/setup/import", func(c *gin.Context) { handlers.ImportDatabase(svc.Server, c) })
		api.POST("/auth/login", func(c *gin.Context) { handlers.Login(svc.Server, c) })
		api.GET("/ws/status", func(c *gin.Context) { handlers.StatusWS(svc.Server, c) })

		protected := api.Group("")
		protected.Use(auth.Middleware(svc.Auth))
		{
			protected.POST("/admin/reset", func(c *gin.Context) { handlers.Reset(svc.Server, c) })
			protected.GET("/settings", func(c *gin.Context) { handlers.GetSettings(svc.Server, c) })
			protected.PUT("/settings", func(c *gin.Context) { handlers.UpdateSettings(svc.Server, c) })
			protected.PUT("/settings/password", func(c *gin.Context) { handlers.ChangePassword(svc.Server, c) })
			protected.GET("/settings/export", func(c *gin.Context) { handlers.ExportDatabase(svc.Server, c) })
			protected.GET("/groups", func(c *gin.Context) { handlers.ListGroups(svc.Server, c) })
			protected.POST("/groups", func(c *gin.Context) { handlers.CreateGroup(svc.Server, c) })
			protected.PUT("/groups/:id", func(c *gin.Context) { handlers.UpdateGroup(svc.Server, c) })
			protected.DELETE("/groups/:id", func(c *gin.Context) { handlers.DeleteGroup(svc.Server, c) })
			protected.GET("/groups/graph", func(c *gin.Context) { handlers.GroupGraph(svc.Server, c) })
			protected.POST("/groups/links", func(c *gin.Context) { handlers.CreateGroupLink(svc.Server, c) })
			protected.DELETE("/groups/links", func(c *gin.Context) { handlers.DeleteGroupLink(svc.Server, c) })
			protected.PUT("/groups/layout", func(c *gin.Context) { handlers.UpdateGroupLayout(svc.Server, c) })
			protected.GET("/forwards", func(c *gin.Context) { handlers.ListPortForwards(svc.Server, c) })
			protected.POST("/forwards", func(c *gin.Context) { handlers.CreatePortForward(svc.Server, c) })
			protected.PUT("/forwards/:id", func(c *gin.Context) { handlers.UpdatePortForward(svc.Server, c) })
			protected.DELETE("/forwards/:id", func(c *gin.Context) { handlers.DeletePortForward(svc.Server, c) })
			protected.GET("/maps", func(c *gin.Context) { handlers.ListMaps(svc.Server, c) })
			protected.POST("/maps", func(c *gin.Context) { handlers.CreateMap(svc.Server, c) })
			protected.PUT("/maps/:id", func(c *gin.Context) { handlers.UpdateMap(svc.Server, c) })
			protected.DELETE("/maps/:id", func(c *gin.Context) { handlers.DeleteMap(svc.Server, c) })
			protected.GET("/peers", func(c *gin.Context) { handlers.ListPeers(svc.Server, c) })
			protected.POST("/peers", func(c *gin.Context) { handlers.CreatePeer(svc.Server, c) })
			protected.PUT("/peers/:id", func(c *gin.Context) { handlers.UpdatePeer(svc.Server, c) })
			protected.DELETE("/peers/:id", func(c *gin.Context) { handlers.DeletePeer(svc.Server, c) })
			protected.POST("/peers/:id/toggle", func(c *gin.Context) { handlers.TogglePeer(svc.Server, c) })
			protected.GET("/peers/:id/config", func(c *gin.Context) { handlers.PeerConfig(svc.Server, c) })
		}
	}
}
