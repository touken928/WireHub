package service

// NetworkRuntime controls the VPN stack lifecycle (implemented by vpn.Stack).
type NetworkRuntime interface {
	Start() error
	Stop() error
	ReloadSettings() error
	SyncPortForwards() error
	HubListenPort() int
}
