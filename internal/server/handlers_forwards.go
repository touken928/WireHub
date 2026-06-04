package server

import (
	"errors"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/filter/l4"
	"gorm.io/gorm"
)

type portForwardResponse struct {
	repo.PortForward
	TargetDisplay string `json:"target_display"`
}

func toPortForwardResponse(f repo.PortForward) portForwardResponse {
	return portForwardResponse{
		PortForward:   f,
		TargetDisplay: domain.ForwardDisplayTarget(f.TargetHost, f.TargetPort),
	}
}

func (s *Server) hubTunnelWebPort() int {
	return l4.HubTunnelWebPort
}

func (s *Server) syncPortForwards(c *gin.Context) {
	if err := s.SyncPortForwards(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
}

func (s *Server) handleListPortForwards(c *gin.Context) {
	rules, err := s.Store.ListPortForwards()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	out := make([]portForwardResponse, 0, len(rules))
	for _, r := range rules {
		out = append(out, toPortForwardResponse(r))
	}
	settings, _ := s.Store.GetSettings()
	hubIP := ""
	if settings != nil {
		hubIP = settings.HubIP
	}
	c.JSON(http.StatusOK, gin.H{
		"rules":    out,
		"hub_ip":   hubIP,
		"hub_port": s.hubTunnelWebPort(),
	})
}

type portForwardRequest struct {
	Name       string `json:"name"`
	ListenPort int    `json:"listen_port" binding:"required"`
	Protocol   string `json:"protocol" binding:"required"`
	TargetHost string `json:"target_host" binding:"required"`
	TargetPort int    `json:"target_port" binding:"required"`
}

func (req *portForwardRequest) toInput() repo.PortForwardInput {
	return repo.PortForwardInput{
		Name:       req.Name,
		ListenPort: req.ListenPort,
		Protocol:   req.Protocol,
		TargetHost: req.TargetHost,
		TargetPort: req.TargetPort,
	}
}

func (s *Server) handleCreatePortForward(c *gin.Context) {
	var req portForwardRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	rule, err := s.Store.CreatePortForward(s.hubTunnelWebPort(), req.toInput())
	if err != nil {
		if errors.Is(err, repo.ErrPortForwardConflict) {
			c.JSON(http.StatusConflict, gin.H{"error": "listen port and protocol already in use"})
			return
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	s.syncPortForwards(c)
	if c.Writer.Written() {
		return
	}
	c.JSON(http.StatusCreated, toPortForwardResponse(*rule))
}

func (s *Server) handleUpdatePortForward(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	var req portForwardRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	rule, err := s.Store.UpdatePortForward(id, s.hubTunnelWebPort(), req.toInput())
	if err != nil {
		if errors.Is(err, repo.ErrPortForwardConflict) {
			c.JSON(http.StatusConflict, gin.H{"error": "listen port and protocol already in use"})
			return
		}
		if errors.Is(err, gorm.ErrRecordNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "forward not found"})
			return
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	s.syncPortForwards(c)
	if c.Writer.Written() {
		return
	}
	c.JSON(http.StatusOK, toPortForwardResponse(*rule))
}

func (s *Server) handleDeletePortForward(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.Store.DeletePortForward(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	s.syncPortForwards(c)
	if c.Writer.Written() {
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}
