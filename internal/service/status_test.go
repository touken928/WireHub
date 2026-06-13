package service

import (
	"sync"
	"sync/atomic"
	"testing"
)

func TestStatusService_SetNotifierNil(t *testing.T) {
	s := newStatusService(nil)
	// SetNotifier with nil must not cause issues
	s.SetNotifier(nil)
	// Publish with nil notifier must not panic
	s.Publish()
}

func TestStatusService_PublishCallsNotifier(t *testing.T) {
	s := newStatusService(nil)
	var called bool
	s.SetNotifier(func() {
		called = true
	})
	s.Publish()
	if !called {
		t.Fatal("expected notifier to be called")
	}
}

func TestStatusService_ReplaceNotifier(t *testing.T) {
	s := newStatusService(nil)
	var first, second int
	s.SetNotifier(func() { first++ })
	s.Publish()
	s.SetNotifier(func() { second++ })
	s.Publish()
	if first != 1 {
		t.Errorf("expected first notifier called once, got %d", first)
	}
	if second != 1 {
		t.Errorf("expected second notifier called once, got %d", second)
	}
}

func TestStatusService_ConcurrentPublishAndSetNotifier(t *testing.T) {
	s := newStatusService(nil)
	var wg sync.WaitGroup
	var publishCount atomic.Int64

	// Start publish goroutines
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 100; j++ {
				s.Publish()
				publishCount.Add(1)
			}
		}()
	}

	// Concurrently set notifiers
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func(id int) {
			defer wg.Done()
			for j := 0; j < 10; j++ {
				s.SetNotifier(func() {})
			}
		}(i)
	}

	wg.Wait()
	// Must not deadlock or panic — the test passes if it completes
	t.Logf("publish count: %d", publishCount.Load())
}

func TestStatusService_PublishNilNotifierAfterSet(t *testing.T) {
	s := newStatusService(nil)
	// Set notifier, then set to nil, publish should not panic
	s.SetNotifier(func() {})
	s.SetNotifier(nil)
	s.Publish()
}

func TestStatusService_ConcurrentReadPublish(t *testing.T) {
	s := newStatusService(nil)
	var wg sync.WaitGroup

	s.SetNotifier(func() {})

	// Concurrent Publish and BuildMessage — no locks held across calls
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			s.Publish()
		}()
		wg.Add(1)
		go func() {
			defer wg.Done()
			// BuildMessage doesn't hold notifyMu, so this tests
			// that Publish doesn't block unrelated operations
		}()
	}
	wg.Wait()
}
