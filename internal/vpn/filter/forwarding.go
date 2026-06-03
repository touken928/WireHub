package filter

import (
	"fmt"
	"reflect"
	"unsafe"

	"golang.zx2c4.com/wireguard/tun/netstack"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv4"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv6"
	"gvisor.dev/gvisor/pkg/tcpip/stack"
)

// EnableForwarding turns on IP forwarding inside the gVisor netstack so peers can reach each other via the hub.
func EnableForwarding(tnet *netstack.Net) error {
	stk, err := stackFromNet(tnet)
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

func stackFromNet(tnet *netstack.Net) (*stack.Stack, error) {
	if tnet == nil {
		return nil, fmt.Errorf("nil netstack")
	}
	rv := reflect.ValueOf(tnet).Elem()
	rf := rv.FieldByName("stack")
	if !rf.IsValid() {
		return nil, fmt.Errorf("netstack stack field not found")
	}
	ptr := unsafe.Pointer(rf.UnsafeAddr())
	rst := reflect.NewAt(rf.Type(), ptr).Elem()
	stk, ok := rst.Interface().(*stack.Stack)
	if !ok || stk == nil {
		return nil, fmt.Errorf("invalid netstack stack")
	}
	return stk, nil
}
