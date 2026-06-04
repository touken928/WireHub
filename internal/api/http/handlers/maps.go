package handlers

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/dto"
	"github.com/touken928/wirehub/internal/api/http/httputil"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

type mapRequest struct {
	Name            string `json:"name"`
	Slug            string `json:"slug" binding:"required"`
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

func ListMaps(s *Server, c *gin.Context) {
	maps, err := s.App.ListMapDetails()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	out := make([]dto.MapResponse, 0, len(maps))
	for _, r := range maps {
		out = append(out, dto.ToMapResponse(r))
	}
	c.JSON(http.StatusOK, gin.H{"maps": out})
}

func CreateMap(s *Server, c *gin.Context) {
	var req mapRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	detail, err := s.App.CreateServiceMap(req.toInput())
	if err != nil {
		writeMapErr(c, err)
		return
	}
	c.JSON(http.StatusCreated, dto.ToMapResponse(*detail))
}

func UpdateMap(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	var req mapRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	detail, err := s.App.UpdateServiceMap(id, req.toInput())
	if err != nil {
		writeMapErr(c, err)
		return
	}
	c.JSON(http.StatusOK, dto.ToMapResponse(*detail))
}

func DeleteMap(s *Server, c *gin.Context) {
	id, err := httputil.ParseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.App.DeleteServiceMap(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func writeMapErr(c *gin.Context, err error) {
	if service.ClassifyMapErr(err) {
		c.JSON(http.StatusConflict, gin.H{"error": err.Error()})
		return
	}
	if err.Error() == "allowed group not found" {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
}
