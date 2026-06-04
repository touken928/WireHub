package policy

import "net/netip"

// TransparentPeer registers a peer WG IP with its group for hub SNAT relay.
type TransparentPeer struct {
	WGIP    netip.Addr
	GroupID uint
}

// TransparentSpec describes unidirectional group links and peer membership for TUN SNAT.
type TransparentSpec struct {
	Peers    []TransparentPeer
	UniLinks []GroupLinkPair // only Bidirectional==false entries are applied
}

// AccessPolicySpec is portable group/map ACL output (no VPN package types).
type AccessPolicySpec struct {
	Blocked     map[netip.Addr][]netip.Addr
	Transparent TransparentSpec
}
