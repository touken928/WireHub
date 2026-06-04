package filter

import (
	"fmt"

	"github.com/touken928/wirehub/internal/vpn/stackutil"
	"golang.zx2c4.com/wireguard/tun/netstack"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv4"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv6"
)

// EnableForwarding turns on IP forwarding inside the gVisor netstack so peers can reach each other via the hub.
func EnableForwarding(tnet *netstack.Net) error {
	stk, err := stackutil.StackFromNet(tnet)
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
