package api

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/auth"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/store"
	"github.com/touken928/wirehub/internal/wg"
)

type setupStatusResponse struct {
	Configured bool                   `json:"configured"`
	Defaults   setupDefaultsResponse  `json:"defaults"`
}

type setupDefaultsResponse struct {
	Subnet          string   `json:"subnet"`
	AdminUsername   string   `json:"admin_username"`
	MTU             int      `json:"mtu"`
	StatusInterval  int      `json:"status_interval"`
	UpstreamDNS     []string `json:"upstream_dns"`
}

type setupRequest struct {
	Endpoint       string   `json:"endpoint" binding:"required"`
	Subnet         string   `json:"subnet"`
	AdminUsername  string   `json:"admin_username"`
	AdminPassword  string   `json:"admin_password" binding:"required"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

func (s *Server) handleSetupStatus(c *gin.Context) {
	configured, err := s.store.IsConfigured()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, setupStatusResponse{
		Configured: configured,
		Defaults: setupDefaultsResponse{
			Subnet:         config.DefaultSubnet,
			AdminUsername:  config.DefaultAdminUsername,
			MTU:            config.DefaultMTU,
			StatusInterval: config.DefaultStatusInterval,
			UpstreamDNS:    append([]string(nil), config.DefaultUpstreamDNS...),
		},
	})
}

func (s *Server) handleSetup(c *gin.Context) {
	configured, err := s.store.IsConfigured()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if configured {
		c.JSON(http.StatusConflict, gin.H{"error": "already configured"})
		return
	}

	var req setupRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	port := config.DefaultPort
	if p, ok := c.Get("listen_port"); ok {
		if listenPort, ok := p.(int); ok && listenPort > 0 {
			port = listenPort
		}
	}

	priv, pub, err := wg.GenerateKeyPair()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if err := s.store.Setup(store.SetupInput{
		Endpoint:         req.Endpoint,
		Subnet:           req.Subnet,
		AdminUsername:    req.AdminUsername,
		AdminPassword:    req.AdminPassword,
		MTU:              req.MTU,
		StatusInterval:   req.StatusInterval,
		ListenPort:       port,
		ServerPrivateKey: priv,
		ServerPublicKey:  pub,
		UpstreamDNS:      req.UpstreamDNS,
	}); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if s.network == nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "network controller unavailable"})
		return
	}
	if err := s.network.Start(); err != nil {
		_ = s.store.ResetAll()
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	authSvc := c.MustGet("auth").(*auth.Service)
	username := req.AdminUsername
	if username == "" {
		username = config.DefaultAdminUsername
	}
	token, err := authSvc.Login(username, req.AdminPassword)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"token": token})
}

func (s *Server) handleReset(c *gin.Context) {
	if s.network == nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "network controller unavailable"})
		return
	}
	if err := s.network.Stop(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if err := s.store.ResetAll(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}
func (s *Server) handleLogin(c *gin.Context) {
	configured, err := s.store.IsConfigured()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if !configured {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "setup required"})
		return
	}

	var req loginRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	authSvc := c.MustGet("auth").(*auth.Service)
	token, err := authSvc.Login(req.Username, req.Password)
	if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"token": token})
}
