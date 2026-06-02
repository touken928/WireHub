package server

import (
	"net/http"
	"os"
	"path/filepath"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/password"
)

type settingsViewResponse struct {
	Endpoint        string   `json:"endpoint"`
	Subnet          string   `json:"subnet"`
	AdminUsername   string   `json:"admin_username"`
	HubIP           string   `json:"hub_ip"`
	DNSIP           string   `json:"dns_ip"`
	DNSSuffix       string   `json:"dns_suffix"`
	ListenPort      int      `json:"listen_port"`
	ServerPublicKey string   `json:"server_public_key"`
	MTU             int      `json:"mtu"`
	StatusInterval  int      `json:"status_interval"`
	UpstreamDNS     []string `json:"upstream_dns"`
}

type updateSettingsRequest struct {
	ListenPort     int      `json:"listen_port"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

type changePasswordRequest struct {
	CurrentPassword string `json:"current_password" binding:"required"`
	NewPassword     string `json:"new_password" binding:"required"`
}

func (s *Server) handleGetSettings(c *gin.Context) {
	settings, err := s.Store.GetSettings()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	admin, err := s.Store.GetPrimaryAdmin()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, settingsViewResponse{
		Endpoint:        settings.Endpoint,
		Subnet:          settings.WGSubnet,
		AdminUsername:   admin.Username,
		HubIP:           settings.HubIP,
		DNSIP:           settings.DNSIP,
		DNSSuffix:       settings.DNSSuffix,
		ListenPort:      settings.ListenPort,
		ServerPublicKey: settings.ServerPublicKey,
		MTU:             settings.MTU,
		StatusInterval:  settings.StatusInterval,
		UpstreamDNS:     settings.UpstreamDNSOrDefault(),
	})
}

func (s *Server) handleUpdateSettings(c *gin.Context) {
	var req updateSettingsRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	settings, err := s.Store.GetSettings()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	oldMTU := settings.MTU
	if err := s.Store.UpdateMutableSettings(req.MTU, req.StatusInterval, req.ListenPort, req.UpstreamDNS); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	settings, err = s.Store.GetSettings()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	mtuChanged := settings.MTU != oldMTU
	networkReload := mtuChanged

	s.SetDNSUpstream(settings.UpstreamDNSOrDefault())
	s.StopStatusPoller()
	s.StartStatusPoller(settings.StatusInterval)

	net := s.NetworkRuntime()
	if networkReload && net != nil {
		if err := net.ReloadSettings(); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"ok":               true,
		"restart_required": networkReload,
	})
}

func (s *Server) handleChangePassword(c *gin.Context) {
	var req changePasswordRequest
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

	if err := password.Verify(admin.PasswordHash, req.CurrentPassword); err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "current password is incorrect"})
		return
	}
	if err := s.Store.UpdateAdminPassword(admin.ID, req.NewPassword); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) handleExportDatabase(c *gin.Context) {
	c.Header("Content-Type", "application/octet-stream")
	c.Header("Content-Disposition", `attachment; filename="wirehub.db"`)
	c.Status(http.StatusOK)
	if err := s.Store.ExportDatabase(c.Writer); err != nil {
		_ = c.Error(err)
	}
}

func (s *Server) handleImportDatabase(c *gin.Context) {
	configured, err := s.Store.IsConfigured()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if configured {
		c.JSON(http.StatusConflict, gin.H{"error": "hub is already configured; reset before importing a database"})
		return
	}

	file, err := c.FormFile("database")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "database file is required"})
		return
	}
	if filepath.Ext(file.Filename) != ".db" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "file must be a .db SQLite database"})
		return
	}

	dataDir := filepath.Dir(s.Store.DatabasePath())
	if err := os.MkdirAll(dataDir, 0o755); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	tmp, err := os.CreateTemp(dataDir, ".wirehub-upload-*.db")
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath)
	_ = tmp.Close()

	if err := c.SaveUploadedFile(file, tmpPath); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if err := s.Store.ImportDatabase(tmpPath); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if net := s.NetworkRuntime(); net != nil {
		if err := net.Start(); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
	}

	c.JSON(http.StatusOK, gin.H{"ok": true})
}
