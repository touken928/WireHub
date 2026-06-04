package dto

import (
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/service"
)

type PortForwardResponse struct {
	repo.PortForward
	TargetDisplay string `json:"target_display"`
}

func ToPortForwardResponse(f repo.PortForward) PortForwardResponse {
	return PortForwardResponse{
		PortForward:   f,
		TargetDisplay: service.ForwardDisplayTarget(f.TargetHost, f.TargetPort),
	}
}
