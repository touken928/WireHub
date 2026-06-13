package handlers

import (
	"fmt"
	"net/http"
	"os"
	"path/filepath"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/dto"
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
)

type updateSettingsRequest struct {
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

type changePasswordRequest struct {
	CurrentPassword string `json:"current_password" binding:"required"`
	NewPassword     string `json:"new_password" binding:"required"`
}

func GetSettings(s *Server, c *gin.Context) {
	view, err := s.App.GetSettingsView()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, dto.ToSettingsViewResponse(view))
}

func UpdateSettings(s *Server, c *gin.Context) {
	var req updateSettingsRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	result, err := s.App.UpdateMutableSettings(req.MTU, req.StatusInterval, req.UpstreamDNS)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"ok":               true,
		"restart_required": result.RestartRequired,
	})
}

func ChangePassword(s *Server, c *gin.Context) {
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
	admin, err := s.App.GetAdminByUsername(username.(string))
	if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "admin not found"})
		return
	}
	if err := repo.VerifyPassword(admin.PasswordHash, req.CurrentPassword); err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "current password is incorrect"})
		return
	}
	if err := s.App.UpdateAdminPassword(admin.ID, req.NewPassword); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func ExportDatabase(s *Server, c *gin.Context) {
	c.Header("Content-Type", "application/octet-stream")
	c.Header("Content-Disposition", `attachment; filename="wirehub.db"`)
	c.Status(http.StatusOK)
	if err := s.App.ExportDatabase(c.Writer); err != nil {
		_ = c.Error(err)
	}
}

func ImportDatabase(s *Server, c *gin.Context) {
	if !requireLocalSetupOrigin(s, c) {
		return
	}
	configured, err := s.App.IsConfigured()
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
	if file.Size > int64(config.MaxUploadBytes) {
		c.JSON(http.StatusRequestEntityTooLarge, gin.H{
			"error": fmt.Sprintf("database file exceeds %d bytes limit", config.MaxUploadBytes),
		})
		return
	}
	dataDir, err := s.App.PrepareDBUploadDir()
	if err != nil {
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
	if err := s.App.ImportDatabase(tmpPath); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}
