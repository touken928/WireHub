package server

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/auth"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/password"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/wg"
)

type setupStatusResponse struct {
	Configured bool                  `json:"configured"`
	Defaults   setupDefaultsResponse `json:"defaults"`
}

type setupDefaultsResponse struct {
	Subnet         string   `json:"subnet"`
	AdminUsername  string   `json:"admin_username"`
	ListenPort     int      `json:"listen_port"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

type setupRequest struct {
	Endpoint       string   `json:"endpoint" binding:"required"`
	Subnet         string   `json:"subnet"`
	AdminUsername  string   `json:"admin_username"`
	AdminPassword  string   `json:"admin_password" binding:"required"`
	ListenPort     int      `json:"listen_port"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

type loginRequest struct {
	Username string `json:"username" binding:"required"`
	Password string `json:"password" binding:"required"`
}

func (s *Server) handleSetupStatus(c *gin.Context) {
	configured, err := s.Store.IsConfigured()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, setupStatusResponse{
		Configured: configured,
		Defaults: setupDefaultsResponse{
			Subnet:         config.DefaultSubnet,
			AdminUsername:  config.DefaultAdminUsername,
			ListenPort:     config.DefaultListenPort,
			MTU:            config.DefaultMTU,
			StatusInterval: config.DefaultStatusInterval,
			UpstreamDNS:    append([]string(nil), config.DefaultUpstreamDNS...),
		},
	})
}

func (s *Server) handleSetup(c *gin.Context) {
	configured, err := s.Store.IsConfigured()
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

	listenPort := req.ListenPort
	if listenPort == 0 {
		listenPort = config.DefaultListenPort
	}

	priv, pub, err := wg.GenerateKeyPair()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if err := s.Store.Setup(repo.SetupInput{
		Endpoint:         req.Endpoint,
		Subnet:           req.Subnet,
		AdminUsername:    req.AdminUsername,
		AdminPassword:    req.AdminPassword,
		MTU:              req.MTU,
		StatusInterval:   req.StatusInterval,
		ListenPort:       listenPort,
		ServerPrivateKey: priv,
		ServerPublicKey:  pub,
		UpstreamDNS:      req.UpstreamDNS,
	}); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	net := s.NetworkRuntime()
	if net == nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "network runtime unavailable"})
		return
	}
	if err := net.Start(); err != nil {
		_ = s.Store.ResetAll()
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

type resetRequest struct {
	Password string `json:"password" binding:"required"`
}

func (s *Server) handleReset(c *gin.Context) {
	var req resetRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	username, ok := c.Get("username")
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
		return
	}
	admin, err := s.Store.GetAdminByUsername(username.(string))
	if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "admin not found"})
		return
	}
	if err := password.Verify(admin.PasswordHash, req.Password); err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "password is incorrect"})
		return
	}

	net := s.NetworkRuntime()
	if net == nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "network runtime unavailable"})
		return
	}
	if err := net.Stop(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if err := s.Store.ResetAll(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) handleLogin(c *gin.Context) {
	configured, err := s.Store.IsConfigured()
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
