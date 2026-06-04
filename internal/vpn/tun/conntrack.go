package tun

import (
	"net/netip"
	"sync"
	"time"
)

type flow4 struct {
	src netip.Addr
	dst netip.Addr
}

type connTrack struct {
	mu    sync.Mutex
	flows map[flow4]time.Time
	ttl   time.Duration
}

func newConnTrack(ttl time.Duration) *connTrack {
	return &connTrack{
		flows: make(map[flow4]time.Time),
		ttl:   ttl,
	}
}

func (ct *connTrack) remember(src, dst netip.Addr) {
	ct.mu.Lock()
	defer ct.mu.Unlock()
	now := time.Now()
	ct.flows[flow4{src, dst}] = now
	ct.gcLocked(now)
}

// allowsReturn is true when an earlier allowed packet went dst→src (reply to that flow).
func (ct *connTrack) allowsReturn(src, dst netip.Addr) bool {
	ct.mu.Lock()
	defer ct.mu.Unlock()
	exp, ok := ct.flows[flow4{dst, src}]
	if !ok {
		return false
	}
	if time.Since(exp) > ct.ttl {
		delete(ct.flows, flow4{dst, src})
		return false
	}
	return true
}

func (ct *connTrack) gcLocked(now time.Time) {
	for k, exp := range ct.flows {
		if now.Sub(exp) > ct.ttl {
			delete(ct.flows, k)
		}
	}
}

func (ct *connTrack) reset() {
	ct.mu.Lock()
	ct.flows = make(map[flow4]time.Time)
	ct.mu.Unlock()
}
