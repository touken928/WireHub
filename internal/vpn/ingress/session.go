package ingress

import (
	"net/netip"
	"time"
)

const (
	protoUDP    = 17
	SessionIdle = 2 * time.Minute
)

type flowKey struct {
	client     netip.Addr
	server     netip.Addr
	clientPort uint16
	serverPort uint16
	proto      uint8
}
