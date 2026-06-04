package dto

import "github.com/touken928/wirehub/internal/service"

type SettingsViewResponse struct {
	Endpoint        string   `json:"endpoint"`
	Subnet          string   `json:"subnet"`
	AdminUsername   string   `json:"admin_username"`
	HubIP           string   `json:"hub_ip"`
	DNSIP           string   `json:"dns_ip"`
	DNSSuffix       string   `json:"dns_suffix"`
	ListenPort      int      `json:"listen_port"`
	ServerPublicKey string   `json:"server_public_key"`
	MTU             int      `json:"mtu"`
	StatusInterval  int      `json:"status_interval"`
	UpstreamDNS     []string `json:"upstream_dns"`
}

func ToSettingsViewResponse(v service.SettingsView) SettingsViewResponse {
	return SettingsViewResponse{
		Endpoint:        v.Endpoint,
		Subnet:          v.Subnet,
		AdminUsername:   v.AdminUsername,
		HubIP:           v.HubIP,
		DNSIP:           v.DNSIP,
		DNSSuffix:       v.DNSSuffix,
		ListenPort:      v.ListenPort,
		ServerPublicKey: v.ServerPublicKey,
		MTU:             v.MTU,
		StatusInterval:  v.StatusInterval,
		UpstreamDNS:     v.UpstreamDNS,
	}
}
