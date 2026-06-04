package dto

import (
	"github.com/touken928/wirehub/internal/domain/map"
	"github.com/touken928/wirehub/internal/repo"
)

type MapResponse struct {
	repo.MapDetail
	FQDN string `json:"fqdn"`
}

func ToMapResponse(d repo.MapDetail) MapResponse {
	return MapResponse{
		MapDetail: d,
		FQDN:      mapdom.MapFQDN(d.Slug),
	}
}
