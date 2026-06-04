package handlers

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/dto"
	"github.com/touken928/wirehub/internal/api/http/httputil"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

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

func ListPortForwards(s *Server, c *gin.Context) {
	list, err := s.App.ListPortForwards()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	out := make([]dto.PortForwardResponse, 0, len(list.Rules))
	for _, r := range list.Rules {
		out = append(out, dto.ToPortForwardResponse(r))
	}
	c.JSON(http.StatusOK, gin.H{
		"rules":    out,
		"hub_ip":   list.HubIP,
		"hub_port": list.HubPort,
	})
}

func CreatePortForward(s *Server, c *gin.Context) {
	var req portForwardRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	rule, err := s.App.CreatePortForward(req.toInput())
	if err != nil {
		writeForwardErr(c, err)
		return
	}
	c.JSON(http.StatusCreated, dto.ToPortForwardResponse(*rule))
}

func UpdatePortForward(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	var req portForwardRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	rule, err := s.App.UpdatePortForward(id, req.toInput())
	if err != nil {
		writeForwardErr(c, err)
		return
	}
	c.JSON(http.StatusOK, dto.ToPortForwardResponse(*rule))
}

func DeletePortForward(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.App.DeletePortForward(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func writeForwardErr(c *gin.Context, err error) {
	switch service.ClassifyForwardErr(err) {
	case service.ForwardErrConflict:
		c.JSON(http.StatusConflict, gin.H{"error": "listen port and protocol already in use"})
	case service.ForwardErrNotFound:
		c.JSON(http.StatusNotFound, gin.H{"error": "forward not found"})
	default:
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
	}
}
