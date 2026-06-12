package dto

import (
	"github.com/touken928/wirehub/internal/domain/peer"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

type PeerResponse struct {
	repo.Peer
	FQDN      string `json:"fqdn"`
	GroupName string `json:"group_name,omitempty"`
}

func ToPeerResponse(p repo.Peer) PeerResponse {
	return PeerResponse{
		Peer: p,
		FQDN: peer.PeerFQDN(p.Name),
	}
}

func EnrichPeerResponse(app *service.App, p repo.Peer) PeerResponse {
	resp := ToPeerResponse(p)
	if g, err := app.GetGroup(p.GroupID); err == nil {
		resp.GroupName = g.Name
	}
	return resp
}

func ToPeerResponses(app *service.App, peers []repo.Peer) []PeerResponse {
	groupNames := app.GetGroupNameMap()
	out := make([]PeerResponse, 0, len(peers))
	for _, p := range peers {
		resp := ToPeerResponse(p)
		resp.GroupName = groupNames[p.GroupID]
		out = append(out, resp)
	}
	return out
}
