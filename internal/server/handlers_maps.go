package server

import (
	"errors"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
)

type mapResponse struct {
	repo.MapDetail
	FQDN string `json:"fqdn"`
}

func toMapResponse(d repo.MapDetail) mapResponse {
	return mapResponse{
		MapDetail: d,
		FQDN:        domain.MapFQDN(d.Slug),
	}
}

func (s *Server) syncMaps(c *gin.Context) {
	if err := s.SyncMaps(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
}

func (s *Server) handleListMaps(c *gin.Context) {
	maps, err := s.Store.ListMapDetails()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	out := make([]mapResponse, 0, len(maps))
	for _, r := range maps {
		out = append(out, toMapResponse(r))
	}
	c.JSON(http.StatusOK, gin.H{"maps": out})
}

type mapRequest struct {
	Name              string `json:"name"`
	Slug              string `json:"slug" binding:"required"`
	TargetHost      string `json:"target_host" binding:"required"`
	AllowedGroupIDs []uint `json:"allowed_group_ids" binding:"required"`
}

func (req *mapRequest) toInput() repo.MapInput {
	return repo.MapInput{
		Name:          req.Name,
		Slug:          req.Slug,
		TargetHost:    req.TargetHost,
		AllowedGroups: req.AllowedGroupIDs,
	}
}

func (s *Server) handleCreateMap(c *gin.Context) {
	var req mapRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	for _, gid := range req.AllowedGroupIDs {
		if _, err := s.Store.GetGroup(gid); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "allowed group not found"})
			return
		}
	}
	detail, err := s.Store.CreateServiceMap(req.toInput())
	if err != nil {
		if errors.Is(err, repo.ErrMapSlugConflict) {
			c.JSON(http.StatusConflict, gin.H{"error": err.Error()})
			return
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	s.SyncMaps()
	c.JSON(http.StatusCreated, toMapResponse(*detail))
}

func (s *Server) handleUpdateMap(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	var req mapRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	for _, gid := range req.AllowedGroupIDs {
		if _, err := s.Store.GetGroup(gid); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "allowed group not found"})
			return
		}
	}
	detail, err := s.Store.UpdateServiceMap(id, req.toInput())
	if err != nil {
		if errors.Is(err, repo.ErrMapSlugConflict) {
			c.JSON(http.StatusConflict, gin.H{"error": err.Error()})
			return
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	s.SyncMaps()
	c.JSON(http.StatusOK, toMapResponse(*detail))
}

func (s *Server) handleDeleteMap(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.Store.DeleteServiceMap(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	s.SyncMaps()
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) SyncMaps() error {
	if err := s.Hub.SyncMaps(); err != nil {
		return err
	}
	return nil
}
