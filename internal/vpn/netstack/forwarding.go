package netstack

import (
	"fmt"

	wgnetstack "golang.zx2c4.com/wireguard/tun/netstack"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv4"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv6"
)

// EnableForwarding turns on IP forwarding inside the gVisor netstack so peers can reach each other via the hub.
func EnableForwarding(tnet *wgnetstack.Net) error {
	stk, err := StackFromNet(tnet)
	if err != nil {
		return err
	}
	if err := stk.SetForwardingDefaultAndAllNICs(ipv4.ProtocolNumber, true); err != nil {
		return fmt.Errorf("enable ipv4 forwarding: %v", err)
	}
	if err := stk.SetForwardingDefaultAndAllNICs(ipv6.ProtocolNumber, true); err != nil {
		return fmt.Errorf("enable ipv6 forwarding: %v", err)
	}
	return nil
}
