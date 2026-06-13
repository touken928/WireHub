package service

import (
	"sync"
	"testing"
	"time"
)

// countingPublisher implements StatusPublisher with a call counter.
type countingPublisher struct {
	mu    sync.Mutex
	count int
}

func (c *countingPublisher) Publish() {
	c.mu.Lock()
	c.count++
	c.mu.Unlock()
}

func (c *countingPublisher) Count() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.count
}

func TestStopStatusPoller_Idempotent(t *testing.T) {
	h := &Hub{}
	// Multiple stops must not panic
	h.StopStatusPoller()
	h.StopStatusPoller()
	// Start then stop
	h.StartStatusPoller(3600) // long interval so it never fires during test
	h.StopStatusPoller()
	h.StopStatusPoller() // second stop must be safe
}

func TestStartStatusPoller_Idempotent(t *testing.T) {
	h := &Hub{}
	// Double start must not panic or leave dangling goroutines
	h.StartStatusPoller(3600)
	h.StartStatusPoller(3600) // no-op, must not panic
	h.StopStatusPoller()
	// One stop is sufficient regardless of how many starts
}

func TestStartStopStatusPoller_Restart(t *testing.T) {
	h := &Hub{}
	// Start with short interval, stop, restart — must not panic or deadlock
	h.StartStatusPoller(1)
	time.Sleep(200 * time.Millisecond)
	h.StopStatusPoller()

	// Restart
	h.StartStatusPoller(1)
	time.Sleep(200 * time.Millisecond)
	h.StopStatusPoller()
}

func TestStartStopStatusPoller_ConcurrentStop(t *testing.T) {
	h := &Hub{}
	var wg sync.WaitGroup

	// Start poller once
	h.StartStatusPoller(1)

	// Concurrent stop calls — must not panic
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			h.StopStatusPoller()
		}()
	}
	wg.Wait()

	// Must be able to restart after concurrent stop
	h.StartStatusPoller(1)
	h.StopStatusPoller()
}

func TestStartStatusPoller_ConcurrentStart(t *testing.T) {
	h := &Hub{}
	var wg sync.WaitGroup

	// Concurrent start calls must not create multiple pollers
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			h.StartStatusPoller(1)
		}()
	}
	wg.Wait()

	// Stop once — if idempotent, one stop is enough
	h.StopStatusPoller()
}

func TestStartStopStatusPoller_Race(t *testing.T) {
	h := &Hub{}
	var wg sync.WaitGroup

	// Start and stop racing against each other
	for i := 0; i < 50; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			h.StartStatusPoller(1)
		}()
		wg.Add(1)
		go func() {
			defer wg.Done()
			h.StopStatusPoller()
		}()
	}
	wg.Wait()
	// Must not deadlock or panic
	h.StopStatusPoller()
}

func TestHubDataplane_NilSafe(t *testing.T) {
	h := &Hub{}
	dp := h.dataplane()
	if dp != nil {
		t.Fatal("expected nil dataplane on fresh Hub")
	}
	// onStopped with no prior start must be safe
	h.onStopped()
	if nc := h.NetworkRuntime(); nc != nil {
		t.Fatal("expected nil NetworkRuntime on fresh Hub")
	}
}

func TestHubStatusPoller_UnsafeInterval(t *testing.T) {
	h := &Hub{}
	// Zero interval should not panic (previously caused NewTicker panic)
	h.StartStatusPoller(0)
	h.StopStatusPoller()
	// Negative should not panic
	h.StartStatusPoller(-1)
	h.StopStatusPoller()
}

func TestHubNetworkRuntime_NilSafe(t *testing.T) {
	h := &Hub{}
	// SyncPortForwards on nil network should not panic or return error
	if err := h.SyncPortForwards(); err != nil {
		t.Fatalf("SyncPortForwards on nil network should return nil, got %v", err)
	}
	// SetDNSUpstream on nil network must not panic
	h.SetDNSUpstream(nil)
	// NetworkRuntime on fresh hub must be nil
	if nc := h.NetworkRuntime(); nc != nil {
		t.Fatal("expected nil NetworkRuntime on fresh Hub")
	}
}

// Test that the inner poller goroutine does not panic with a nil dataplane.
func TestHubPollPeerStats_NilDataplane(t *testing.T) {
	h := &Hub{}
	// pollPeerStats should handle nil dataplane without panic
	h.pollPeerStats()
}

// race-free baseline for go test -race.
func TestStatusPoller_NoRace(t *testing.T) {
	h := &Hub{}
	h.SetStatusPublisher(&countingPublisher{})
	h.StartStatusPoller(3600)
	// Read dataplane concurrently
	go func() {
		_ = h.dataplane()
	}()
	// Read network concurrently
	go func() {
		_ = h.NetworkRuntime()
	}()
	time.Sleep(10 * time.Millisecond)
	h.StopStatusPoller()
}


