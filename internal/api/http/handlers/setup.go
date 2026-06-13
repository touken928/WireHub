package handlers

import (
	"errors"
	"net"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/auth"
	"github.com/touken928/wirehub/internal/api/http/dto"
	"github.com/touken928/wirehub/internal/api/http/httputil"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/service"
)

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

type resetRequest struct {
	Password string `json:"password" binding:"required"`
}

type loginRequest struct {
	Username string `json:"username" binding:"required"`
	Password string `json:"password" binding:"required"`
}

func SetupStatus(s *Server, c *gin.Context) {
	if !requireLocalSetupOrigin(s, c) {
		return
	}
	configured, defaults, err := s.App.SetupStatus()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, dto.ToSetupStatusResponse(configured, defaults))
}

func Setup(s *Server, c *gin.Context) {
	if !requireLocalSetupOrigin(s, c) {
		return
	}
	configured, err := s.App.IsConfigured()
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
	err = s.App.Setup(service.SetupInput{
		Endpoint:       req.Endpoint,
		Subnet:         req.Subnet,
		AdminUsername:  req.AdminUsername,
		AdminPassword:  req.AdminPassword,
		ListenPort:     req.ListenPort,
		MTU:            req.MTU,
		StatusInterval: req.StatusInterval,
		UpstreamDNS:    req.UpstreamDNS,
	})
	if err != nil {
		if errors.Is(err, service.ErrAlreadyConfigured) {
			c.JSON(http.StatusConflict, gin.H{"error": err.Error()})
			return
		}
		status := http.StatusBadRequest
		if errors.Is(err, service.ErrNetworkUnavailable) {
			status = http.StatusInternalServerError
		}
		c.JSON(status, gin.H{"error": err.Error()})
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

func Reset(s *Server, c *gin.Context) {
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
	if _, err := s.App.VerifyAdminPassword(username.(string), req.Password); errors.Is(err, service.ErrInvalidAdminPassword) {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "password is incorrect"})
		return
	} else if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "admin not found"})
		return
	}
	if err := s.App.Reset(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

// requireLocalSetupOrigin rejects non-loopback requests when the hub is unconfigured,
// unless AllowRemoteSetup is explicitly enabled. This prevents remote claim of fresh
// public deployments without relying on spoofable Origin/Host headers.
func requireLocalSetupOrigin(s *Server, c *gin.Context) bool {
	if s.AllowRemoteSetup {
		return true
	}
	configured, err := s.App.IsConfigured()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return false
	}
	if configured {
		return true // already set up; auth protects the rest
	}
	host, _, err := net.SplitHostPort(c.Request.RemoteAddr)
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "setup must be performed from localhost"})
		return false
	}
	ip := net.ParseIP(host)
	if ip == nil || !ip.IsLoopback() {
		c.JSON(http.StatusForbidden, gin.H{"error": "setup must be performed from localhost"})
		return false
	}
	return true
}

func Login(s *Server, c *gin.Context) {
	configured, err := s.App.IsConfigured()
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
		c.JSON(http.StatusBadRequest, gin.H{"error": "username and password are required"})
		return
	}
	ip := httputil.ClientIP(c)
	if httputil.RejectLoginRateLimit(c, s.LoginLimiter(), ip) {
		return
	}
	authSvc := c.MustGet("auth").(*auth.Service)
	token, err := authSvc.Login(req.Username, req.Password)
	if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": err.Error()})
		return
	}
	if lim := s.LoginLimiter(); lim != nil {
		lim.RecordLoginSuccess(ip)
	}
	c.JSON(http.StatusOK, gin.H{"token": token})
}
