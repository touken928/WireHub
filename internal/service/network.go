package service

import (
	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	vpnruntime "github.com/touken928/wirehub/internal/vpn/runtime"
)

// NetworkRuntime controls VPN stack lifecycle.
type NetworkRuntime interface {
	Start(bundle domainruntime.SyncBundle) error
	Stop() error
	ReloadSettings() error
	SyncPortForwards() error
	SyncMaps() error
	HubListenPort() int
	SetDNSUpstream(upstream []string)
}

// Dataplane is the live VPN data plane (alias of runtime.Dataplane).
type Dataplane = vpnruntime.Dataplane
