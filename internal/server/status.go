package server

import (
	"encoding/json"
	"time"

	"github.com/touken928/wirehub/internal/domain"
)

type statusPeerView struct {
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

type statusMessage struct {
	Type     string           `json:"type"`
	Peers    []statusPeerView `json:"peers"`
	Settings interface{}      `json:"settings"`
}

func (s *Server) buildStatusMessage() (statusMessage, error) {
	peers, err := s.Store.ListPeers()
	if err != nil {
		return statusMessage{}, err
	}
	settings, err := s.Store.GetSettings()
	if err != nil {
		return statusMessage{}, err
	}
	groups, _ := s.Store.ListGroups()
	groupNames := map[uint]string{}
	for _, g := range groups {
		groupNames[g.ID] = g.Name
	}

	now := time.Now()
	result := make([]statusPeerView, 0, len(peers))
	for _, p := range peers {
		online := domain.PeerIsOnline(p.Enabled, p.LastHandshake, now)
		result = append(result, statusPeerView{
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
	return statusMessage{
		Type:     "status",
		Peers:    result,
		Settings: settings,
	}, nil
}

func (s *Server) buildStatusJSON() ([]byte, error) {
	msg, err := s.buildStatusMessage()
	if err != nil {
		return nil, err
	}
	return json.Marshal(msg)
}

func (s *Server) publishStatus() {
	if s.statusHub != nil {
		s.statusHub.Publish()
	}
}
