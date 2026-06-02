package api

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/hostname"
	"github.com/touken928/wirehub/internal/store"
)

type loginRequest struct {
	Username string `json:"username" binding:"required"`
	Password string `json:"password" binding:"required"`
}

type peerResponse struct {
	store.Peer
	FQDN      string `json:"fqdn"`
	GroupName string `json:"group_name,omitempty"`
}

func toPeerResponse(p store.Peer) peerResponse {
	return peerResponse{
		Peer: p,
		FQDN: hostname.FQDN(p.Name),
	}
}

func (s *Server) enrichPeerResponse(p store.Peer) peerResponse {
	resp := toPeerResponse(p)
	if g, err := s.store.GetGroup(p.GroupID); err == nil {
		resp.GroupName = g.Name
	}
	return resp
}

func toPeerResponses(st *store.Store, peers []store.Peer) []peerResponse {
	out := make([]peerResponse, 0, len(peers))
	for _, p := range peers {
		resp := toPeerResponse(p)
		if g, err := st.GetGroup(p.GroupID); err == nil {
			resp.GroupName = g.Name
		}
		out = append(out, resp)
	}
	return out
}

func (s *Server) handleStatus(c *gin.Context) {
	peers, err := s.store.ListPeers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	settings, _ := s.store.GetSettings()
	groups, _ := s.store.ListGroups()
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
			FQDN:          hostname.FQDN(p.Name),
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
	peers, err := s.store.ListPeers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if peers == nil {
		peers = emptySlice[store.Peer]()
	}
	c.JSON(http.StatusOK, toPeerResponses(s.store, peers))
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

	slug, err := store.ValidateHostname(req.Name)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if _, err := s.store.GetGroup(req.GroupID); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "group not found"})
		return
	}

	existing, _ := s.store.ListPeers()
	for _, p := range existing {
		if p.Name == slug {
			c.JSON(http.StatusBadRequest, gin.H{"error": "hostname already exists"})
			return
		}
	}

	settings, err := s.store.GetSettings()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	priv, pub, err := wgGenerateKeyPair()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	ip, err := s.store.AllocateIP(settings.WGSubnet, settings.HubIP)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	peer := &store.Peer{
		Name:       slug,
		PublicKey:  pub,
		PrivateKey: priv,
		WGIP:       ip,
		GroupID:    req.GroupID,
		Enabled:    true,
		DNSName:    slug,
	}

	if err := s.store.CreatePeer(peer); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	wgMgr, err := s.wgMgr()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": err.Error()})
		return
	}
	if err := wgMgr.SyncPeer(peer); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if dns, err := s.dnsServer(); err == nil {
		_ = dns.RegisterPeer(peer)
	}
	_ = s.store.UpdatePeer(peer)
	s.syncAccessFilter()
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
	peer, err := s.store.GetPeer(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "peer not found"})
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
	if _, err := s.store.GetGroup(*req.GroupID); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "group not found"})
		return
	}
	peer.GroupID = *req.GroupID

	if err := s.store.UpdatePeer(peer); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	wgMgr, err := s.wgMgr()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": err.Error()})
		return
	}
	if err := wgMgr.SyncPeer(peer); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	s.syncAccessFilter()
	c.JSON(http.StatusOK, s.enrichPeerResponse(*peer))
}

func (s *Server) handleDeletePeer(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	peer, err := s.store.GetPeer(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "peer not found"})
		return
	}
	if wgMgr, err := s.wgMgr(); err == nil {
		_ = wgMgr.RemovePeer(peer.PublicKey)
	}
	_ = s.store.DeleteDNSByPeerID(id)
	if err := s.store.DeletePeer(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	s.syncAccessFilter()
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) handleTogglePeer(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	peer, err := s.store.GetPeer(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "peer not found"})
		return
	}
	peer.Enabled = !peer.Enabled
	if err := s.store.UpdatePeer(peer); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	wgMgr, err := s.wgMgr()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": err.Error()})
		return
	}
	if peer.Enabled {
		if err := wgMgr.SyncPeer(peer); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
	} else {
		_ = wgMgr.RemovePeer(peer.PublicKey)
	}
	s.syncAccessFilter()
	c.JSON(http.StatusOK, s.enrichPeerResponse(*peer))
}

func (s *Server) handlePeerConfig(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	peer, err := s.store.GetPeer(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "peer not found"})
		return
	}
	settings, err := s.store.GetSettings()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	conf, err := buildClientConfig(settings, peer)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"config": conf, "filename": peer.Name + ".conf"})
}
