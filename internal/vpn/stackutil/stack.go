package stackutil

import (
	"fmt"
	"reflect"
	"unsafe"

	"golang.zx2c4.com/wireguard/tun/netstack"
	"gvisor.dev/gvisor/pkg/tcpip/stack"
)

// StackFromNet exposes the gVisor stack inside wireguard-go netstack.
func StackFromNet(tnet *netstack.Net) (*stack.Stack, error) {
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
