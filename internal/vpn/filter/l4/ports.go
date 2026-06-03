package l4

import (
	"crypto/rand"
	"math/big"
	"time"
)

const (
	// EphemeralPortMin/Max is the hub source port range for transparent SNAT flows.
	EphemeralPortMin = 20000
	EphemeralPortMax = 65000
	// SessionIdle is the idle timeout for transparent SNAT and explicit UDP relay sessions.
	SessionIdle = 2 * time.Minute
)

// ephemeralPortPool allocates random hub ports, skipping reserved listen ports.
type ephemeralPortPool struct {
	reserved map[uint16]struct{}
	inUse    map[uint16]struct{}
}

func newEphemeralPortPool() *ephemeralPortPool {
	return &ephemeralPortPool{
		reserved: make(map[uint16]struct{}),
		inUse:    make(map[uint16]struct{}),
	}
}

func (p *ephemeralPortPool) resetInUse() {
	p.inUse = make(map[uint16]struct{})
}

func (p *ephemeralPortPool) reserveHubPorts(ports []int) {
	for _, port := range ports {
		if port >= 0 && port <= 65535 {
			p.reserved[uint16(port)] = struct{}{}
		}
	}
}

func (p *ephemeralPortPool) pick() (uint16, error) {
	span := int64(EphemeralPortMax - EphemeralPortMin + 1)
	for i := 0; i < 256; i++ {
		n, err := rand.Int(rand.Reader, big.NewInt(span))
		if err != nil {
			return 0, err
		}
		port := uint16(EphemeralPortMin + int(n.Int64()))
		if _, ok := p.reserved[port]; ok {
			continue
		}
		if _, ok := p.inUse[port]; ok {
			continue
		}
		p.inUse[port] = struct{}{}
		return port, nil
	}
	return 0, errPortsExhausted{}
}

type errPortsExhausted struct{}

func (errPortsExhausted) Error() string {
	return "no ephemeral SNAT port available"
}
