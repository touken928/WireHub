package filter

// AccessPolicy is applied on the hub TUN device.
type AccessPolicy struct {
	Rules *RuleSet
	SNAT  *UniSNATTable
}
