package handlers

import (
	"errors"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/dto"
	"github.com/touken928/wirehub/internal/api/http/httputil"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

type createPeerRequest struct {
	Name    string `json:"name" binding:"required"`
	GroupID uint   `json:"group_id" binding:"required"`
}

type updatePeerRequest struct {
	Name    *string `json:"name"`
	GroupID *uint   `json:"group_id"`
}

func ListPeers(s *Server, c *gin.Context) {
	peers, err := s.App.ListPeers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if peers == nil {
		peers = dto.EmptySlice[repo.Peer]()
	}
	c.JSON(http.StatusOK, dto.ToPeerResponses(s.App, peers))
}

func CreatePeer(s *Server, c *gin.Context) {
	var req createPeerRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	peer, err := s.App.CreatePeer(req.Name, req.GroupID)
	if err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, service.ErrNetworkUnavailable) {
			status = http.StatusServiceUnavailable
		} else if errors.Is(err, service.ErrGroupNotFound) || errors.Is(err, service.ErrHostnameExists) {
			status = http.StatusBadRequest
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	s.App.NotifyStatus()
	c.JSON(http.StatusCreated, dto.EnrichPeerResponse(s.App, *peer))
}

func UpdatePeer(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	var req updatePeerRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if req.GroupID == nil && req.Name == nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "name or group_id is required"})
		return
	}
	peer, err := s.App.UpdatePeerFields(id, req.Name, req.GroupID)
	if err != nil {
		writePeerErr(c, err)
		return
	}
	s.App.NotifyStatus()
	c.JSON(http.StatusOK, dto.EnrichPeerResponse(s.App, *peer))
}

func DeletePeer(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.App.DeletePeer(id); err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, service.ErrPeerNotFound) {
			status = http.StatusNotFound
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	s.App.NotifyStatus()
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func TogglePeer(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	peer, err := s.App.TogglePeer(id)
	if err != nil {
		writePeerErr(c, err)
		return
	}
	s.App.NotifyStatus()
	c.JSON(http.StatusOK, dto.EnrichPeerResponse(s.App, *peer))
}

func PeerConfig(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	conf, err := s.App.ClientConfig(id)
	if err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, service.ErrPeerNotFound) {
			status = http.StatusNotFound
		} else if err.Error() == "server endpoint is not configured" {
			status = http.StatusBadRequest
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	peer, _ := s.App.GetPeer(id)
	c.JSON(http.StatusOK, gin.H{"config": conf, "filename": peer.Name + ".conf"})
}

func writePeerErr(c *gin.Context, err error) {
	status := http.StatusBadRequest
	if errors.Is(err, service.ErrPeerNotFound) {
		status = http.StatusNotFound
	} else if errors.Is(err, service.ErrNetworkUnavailable) {
		status = http.StatusServiceUnavailable
	}
	c.JSON(status, gin.H{"error": err.Error()})
}
