package server

import (
	"errors"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/service"
	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
)

type peerResponse struct {
	repo.Peer
	FQDN      string `json:"fqdn"`
	GroupName string `json:"group_name,omitempty"`
}

func toPeerResponse(p repo.Peer) peerResponse {
	return peerResponse{
		Peer: p,
		FQDN: domain.PeerFQDN(p.Name),
	}
}

func (s *Server) enrichPeerResponse(p repo.Peer) peerResponse {
	resp := toPeerResponse(p)
	if g, err := s.Store.GetGroup(p.GroupID); err == nil {
		resp.GroupName = g.Name
	}
	return resp
}

func toPeerResponses(h *service.Hub, peers []repo.Peer) []peerResponse {
	out := make([]peerResponse, 0, len(peers))
	for _, p := range peers {
		resp := toPeerResponse(p)
		if g, err := h.Store.GetGroup(p.GroupID); err == nil {
			resp.GroupName = g.Name
		}
		out = append(out, resp)
	}
	return out
}

func (s *Server) handleStatus(c *gin.Context) {
	peers, err := s.Store.ListPeers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	settings, _ := s.Store.GetSettings()
	groups, _ := s.Store.ListGroups()
	groupNames := map[uint]string{}
	for _, g := range groups {
		groupNames[g.ID] = g.Name
	}
	type peerStatus struct {
		ID            uint   `json:"id"`
		Name          string `json:"name"`
		FQDN          string `json:"fqdn"`
		WGIP          string `json:"wg_ip"`
		GroupID       uint   `json:"group_id"`
		GroupName     string `json:"group_name"`
		Enabled       bool   `json:"enabled"`
		LastHandshake int64  `json:"last_handshake"`
		RxBytes       int64  `json:"rx_bytes"`
		TxBytes       int64  `json:"tx_bytes"`
		Online        bool   `json:"online"`
	}
	now := time.Now()
	result := make([]peerStatus, 0)
	for _, p := range peers {
		online := false
		if p.LastHandshake > 0 {
			online = now.Sub(time.Unix(p.LastHandshake, 0)) < 3*time.Minute
		}
		result = append(result, peerStatus{
			ID:            p.ID,
			Name:          p.Name,
			FQDN:          domain.PeerFQDN(p.Name),
			WGIP:          p.WGIP,
			GroupID:       p.GroupID,
			GroupName:     groupNames[p.GroupID],
			Enabled:       p.Enabled,
			LastHandshake: p.LastHandshake,
			RxBytes:       p.RxBytes,
			TxBytes:       p.TxBytes,
			Online:        online,
		})
	}
	c.JSON(http.StatusOK, gin.H{
		"peers":    result,
		"settings": settings,
	})
}

func (s *Server) handleListPeers(c *gin.Context) {
	peers, err := s.Store.ListPeers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if peers == nil {
		peers = emptySlice[repo.Peer]()
	}
	c.JSON(http.StatusOK, toPeerResponses(s.Hub, peers))
}

type createPeerRequest struct {
	Name    string `json:"name" binding:"required"`
	GroupID uint   `json:"group_id" binding:"required"`
}

func (s *Server) handleCreatePeer(c *gin.Context) {
	var req createPeerRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	peer, err := s.CreatePeer(req.Name, req.GroupID)
	if err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, service.ErrNetworkUnavailable) {
			status = http.StatusServiceUnavailable
		} else if err.Error() == "group not found" || err.Error() == "hostname already exists" {
			status = http.StatusBadRequest
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusCreated, s.enrichPeerResponse(*peer))
}

type updatePeerRequest struct {
	GroupID *uint `json:"group_id"`
}

func (s *Server) handleUpdatePeer(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	var req updatePeerRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if req.GroupID == nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "group_id is required"})
		return
	}

	peer, err := s.UpdatePeerGroup(id, *req.GroupID)
	if err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, service.ErrNetworkUnavailable) {
			status = http.StatusServiceUnavailable
		} else if err.Error() == "peer not found" || err.Error() == "group not found" {
			status = http.StatusNotFound
			if err.Error() == "group not found" {
				status = http.StatusBadRequest
			}
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, s.enrichPeerResponse(*peer))
}

func (s *Server) handleDeletePeer(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.DeletePeer(id); err != nil {
		status := http.StatusInternalServerError
		if err.Error() == "peer not found" {
			status = http.StatusNotFound
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) handleTogglePeer(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	peer, err := s.TogglePeer(id)
	if err != nil {
		status := http.StatusInternalServerError
		if errors.Is(err, service.ErrNetworkUnavailable) {
			status = http.StatusServiceUnavailable
		} else if err.Error() == "peer not found" {
			status = http.StatusNotFound
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, s.enrichPeerResponse(*peer))
}

func (s *Server) handlePeerConfig(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	conf, err := s.ClientConfig(id)
	if err != nil {
		status := http.StatusInternalServerError
		if err.Error() == "peer not found" {
			status = http.StatusNotFound
		} else if err.Error() == "server endpoint is not configured" {
			status = http.StatusBadRequest
		}
		c.JSON(status, gin.H{"error": err.Error()})
		return
	}
	peer, _ := s.Store.GetPeer(id)
	c.JSON(http.StatusOK, gin.H{"config": conf, "filename": peer.Name + ".conf"})
}
