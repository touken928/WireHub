package dto

import "github.com/touken928/wirehub/internal/service"

type SetupStatusResponse struct {
	Configured bool                  `json:"configured"`
	Defaults   SetupDefaultsResponse `json:"defaults"`
}

type SetupDefaultsResponse struct {
	Subnet         string   `json:"subnet"`
	AdminUsername  string   `json:"admin_username"`
	ListenPort     int      `json:"listen_port"`
	MTU            int      `json:"mtu"`
	StatusInterval int      `json:"status_interval"`
	UpstreamDNS    []string `json:"upstream_dns"`
}

func ToSetupStatusResponse(configured bool, d service.SetupDefaults) SetupStatusResponse {
	return SetupStatusResponse{
		Configured: configured,
		Defaults: SetupDefaultsResponse{
			Subnet:         d.Subnet,
			AdminUsername:  d.AdminUsername,
			ListenPort:     d.ListenPort,
			MTU:            d.MTU,
			StatusInterval: d.StatusInterval,
			UpstreamDNS:    d.UpstreamDNS,
		},
	}
}
