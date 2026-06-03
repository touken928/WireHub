package filter

import "github.com/touken928/wirehub/internal/vpn/filter/l4"

// AccessPolicy is applied on the hub TUN device.
type AccessPolicy struct {
	Rules       *RuleSet
	Transparent *l4.TransparentTable
}
