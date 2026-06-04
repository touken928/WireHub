package dto

import "github.com/touken928/wirehub/internal/service"

type GroupResponse struct {
	service.GroupView
}

func ToGroupResponse(v service.GroupView) GroupResponse {
	return GroupResponse{GroupView: v}
}
