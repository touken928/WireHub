// Package integration holds black-box tests against WireHub's VPN data plane
// (wireguard-go netstack, DNS, ACL, ingress forwards/maps).
//
// Layout:
//
//	mesh.go, client.go, sync.go — hub + peer mesh harness
//	dns.go, net.go, host.go     — probes and fixtures
//	*_test.go                   — scenarios by feature (dns, forward, map, …)
package integration
