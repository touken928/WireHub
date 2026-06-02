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
	FQDN string `json:"fqdn"`
}

func toPeerResponse(p store.Peer) peerResponse {
	return peerResponse{
		Peer: p,
		FQDN: hostname.FQDN(p.Name),
	}
}

func toPeerResponses(peers []store.Peer) []peerResponse {
	out := make([]peerResponse, 0, len(peers))
	for _, p := range peers {
		out = append(out, toPeerResponse(p))
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
	type peerStatus struct {
		ID            uint     `json:"id"`
		Name          string   `json:"name"`
		FQDN          string   `json:"fqdn"`
		WGIP          string   `json:"wg_ip"`
		AccessExclude []string `json:"access_exclude"`
		Enabled       bool     `json:"enabled"`
		LastHandshake int64    `json:"last_handshake"`
		RxBytes       int64    `json:"rx_bytes"`
		TxBytes       int64    `json:"tx_bytes"`
		Online        bool     `json:"online"`
	}
	now := time.Now()
	result := make([]peerStatus, 0)
	for _, p := range peers {
		online := false
		if p.LastHandshake > 0 {
			online = now.Sub(time.Unix(p.LastHandshake, 0)) < 3*time.Minute
		}
		exclude := p.AccessExclude
		if exclude == nil {
			exclude = []string{}
		}
		result = append(result, peerStatus{
			ID:            p.ID,
			Name:          p.Name,
			FQDN:          hostname.FQDN(p.Name),
			WGIP:          p.WGIP,
			AccessExclude: exclude,
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
	c.JSON(http.StatusOK, toPeerResponses(peers))
}

type createPeerRequest struct {
	Name          string   `json:"name" binding:"required"`
	AccessExclude []string `json:"access_exclude"`
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

	existing, _ := s.store.ListPeers()
	for _, p := range existing {
		if p.Name == slug {
			c.JSON(http.StatusBadRequest, gin.H{"error": "hostname already exists"})
			return
		}
	}

	exclude := store.ParseExcludeLines(req.AccessExclude)
	if err := validateExclude(existing, store.Peer{Name: slug}, exclude); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
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
		Name:          slug,
		PublicKey:     pub,
		PrivateKey:    priv,
		WGIP:          ip,
		AccessExclude: exclude,
		Enabled:       true,
		DNSName:       slug,
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
	c.JSON(http.StatusCreated, toPeerResponse(*peer))
}

type updatePeerRequest struct {
	AccessExclude []string `json:"access_exclude"`
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

	exclude := store.ParseExcludeLines(req.AccessExclude)
	if exclude == nil {
		exclude = []string{}
	}

	all, _ := s.store.ListPeers()
	if err := validateExclude(all, *peer, exclude); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	peer.AccessExclude = exclude

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
	c.JSON(http.StatusOK, toPeerResponse(*peer))
}

func validateExclude(peers []store.Peer, self store.Peer, lines []string) error {
	for _, line := range lines {
		pattern, _, err := store.ParseExcludePattern(line)
		if err != nil {
			return err
		}
		if err := store.ValidateExcludePattern(pattern); err != nil {
			return err
		}
	}
	_, err := store.ResolveExcludeRules(peers, self, lines)
	return err
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
	c.JSON(http.StatusOK, toPeerResponse(*peer))
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
