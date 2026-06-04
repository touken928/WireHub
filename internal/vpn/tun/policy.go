package tun

import "github.com/touken928/wirehub/internal/vpn/snat"

// AccessPolicy is applied on the hub TUN device.
type AccessPolicy struct {
	Rules       *RuleSet
	Transparent *snat.TransparentTable
}
