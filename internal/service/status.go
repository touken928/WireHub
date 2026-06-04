package service

import (
	"encoding/json"
	"time"

	"github.com/touken928/wirehub/internal/domain/peer"
)

// StatusPeerView is one peer row in a live status snapshot.
type StatusPeerView struct {
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

// StatusMessage is the WebSocket status payload.
type StatusMessage struct {
	Type     string           `json:"type"`
	Peers    []StatusPeerView `json:"peers"`
	Settings interface{}      `json:"settings"`
}

// StatusService builds status snapshots and implements StatusPublisher.
type StatusService struct {
	app      *App
	onNotify func()
}

func newStatusService(app *App) *StatusService {
	return &StatusService{app: app}
}

// SetNotifier wires WebSocket broadcast (called from HTTP layer).
func (s *StatusService) SetNotifier(fn func()) {
	s.onNotify = fn
}

// Publish implements StatusPublisher.
func (s *StatusService) Publish() {
	if s.onNotify != nil {
		s.onNotify()
	}
}

// BuildMessage assembles the current status snapshot.
func (s *StatusService) BuildMessage() (StatusMessage, error) {
	peers, err := s.app.Store.ListPeers()
	if err != nil {
		return StatusMessage{}, err
	}
	settings, err := s.app.Store.GetSettings()
	if err != nil {
		return StatusMessage{}, err
	}
	groups, _ := s.app.Store.ListGroups()
	groupNames := map[uint]string{}
	for _, g := range groups {
		groupNames[g.ID] = g.Name
	}

	now := time.Now()
	result := make([]StatusPeerView, 0, len(peers))
	for _, p := range peers {
		online := peer.PeerIsOnline(p.Enabled, p.LastHandshake, now)
		result = append(result, StatusPeerView{
			ID:            p.ID,
			Name:          p.Name,
			FQDN:          peer.PeerFQDN(p.Name),
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
	return StatusMessage{
		Type:     "status",
		Peers:    result,
		Settings: settings,
	}, nil
}

// BuildJSON marshals the status snapshot for WebSocket clients.
func (s *StatusService) BuildJSON() ([]byte, error) {
	msg, err := s.BuildMessage()
	if err != nil {
		return nil, err
	}
	return json.Marshal(msg)
}

// Notify pushes a fresh snapshot to subscribers.
func (a *App) NotifyStatus() {
	a.Status.Publish()
}
