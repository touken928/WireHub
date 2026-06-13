package httputil

import (
	"testing"
	"time"
)

func TestLoginRateLimiter_TakeUntilEmpty(t *testing.T) {
	now := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	l := NewLoginRateLimiter(LoginRateLimitConfig{Capacity: 5, RefillPeriod: 10 * time.Minute})
	l.now = func() time.Time { return now }

	ip := "203.0.113.10"
	for i := 0; i < 5; i++ {
		if _, ok := l.Take(ip); !ok {
			t.Fatalf("attempt %d: expected allowed", i+1)
		}
	}
	if _, ok := l.Take(ip); ok {
		t.Fatal("sixth attempt should be rejected")
	}
}

func TestLoginRateLimiter_RefillsOverTime(t *testing.T) {
	start := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	current := start
	l := NewLoginRateLimiter(LoginRateLimitConfig{Capacity: 5, RefillPeriod: 10 * time.Minute})
	l.now = func() time.Time { return current }

	ip := "203.0.113.11"
	for i := 0; i < 5; i++ {
		if _, ok := l.Take(ip); !ok {
			t.Fatalf("attempt %d: expected allowed", i+1)
		}
	}
	if _, ok := l.Take(ip); ok {
		t.Fatal("expected bucket empty")
	}

	current = start.Add(2 * time.Minute)
	if _, ok := l.Take(ip); !ok {
		t.Fatal("expected one token after 2 minutes")
	}
	if _, ok := l.Take(ip); ok {
		t.Fatal("expected empty again after single refill token")
	}
}

func TestLoginRateLimiter_RecordLoginSuccessClearsBucket(t *testing.T) {
	now := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	l := NewLoginRateLimiter(LoginRateLimitConfig{Capacity: 5, RefillPeriod: 10 * time.Minute})
	l.now = func() time.Time { return now }

	ip := "203.0.113.12"
	for i := 0; i < 3; i++ {
		if _, ok := l.Take(ip); !ok {
			t.Fatalf("attempt %d: expected allowed", i+1)
		}
	}
	l.RecordLoginSuccess(ip)

	for i := 0; i < 5; i++ {
		if _, ok := l.Take(ip); !ok {
			t.Fatalf("after reset attempt %d: expected allowed", i+1)
		}
	}
}

func TestLoginRateLimiter_RetryAfterWhenEmpty(t *testing.T) {
	now := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	l := NewLoginRateLimiter(LoginRateLimitConfig{Capacity: 5, RefillPeriod: 10 * time.Minute})
	l.now = func() time.Time { return now }

	ip := "203.0.113.13"
	for i := 0; i < 5; i++ {
		l.Take(ip)
	}
	retry, ok := l.Take(ip)
	if ok {
		t.Fatal("expected rejected")
	}
	want := 2 * time.Minute
	if retry != want {
		t.Fatalf("retryAfter = %v, want %v", retry, want)
	}
}

func TestLoginRateLimiter_EmptyIPAlwaysAllowed(t *testing.T) {
	l := DefaultLoginRateLimiter()
	for i := 0; i < 10; i++ {
		if _, ok := l.Take(""); !ok {
			t.Fatal("empty ip should not be limited")
		}
	}
}

func TestDefaultLoginRateLimiterConfig(t *testing.T) {
	l := DefaultLoginRateLimiter()
	if l.cfg.Capacity != loginRateCapacity {
		t.Fatalf("capacity = %v, want %v", l.cfg.Capacity, loginRateCapacity)
	}
	if l.cfg.RefillPeriod != loginRateRefillPeriod {
		t.Fatalf("refill = %v, want %v", l.cfg.RefillPeriod, loginRateRefillPeriod)
	}
	if l.cfg.MaxEntries != loginRateMaxEntries {
		t.Fatalf("MaxEntries = %v, want %v", l.cfg.MaxEntries, loginRateMaxEntries)
	}
	if l.cfg.CleanupInterval != loginRateCleanupInterval {
		t.Fatalf("CleanupInterval = %v, want %v", l.cfg.CleanupInterval, loginRateCleanupInterval)
	}
	if l.cfg.StaleTTL != loginRateStaleTTL {
		t.Fatalf("StaleTTL = %v, want %v", l.cfg.StaleTTL, loginRateStaleTTL)
	}
}

func TestLoginRateLimiter_MaxEntriesEviction(t *testing.T) {
	now := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	l := NewLoginRateLimiter(LoginRateLimitConfig{
		Capacity:     5,
		RefillPeriod: 10 * time.Minute,
		MaxEntries:   2,
	})
	l.now = func() time.Time { return now }

	// Fill with IP1 and IP2.
	if _, ok := l.Take("10.0.0.1"); !ok {
		t.Fatal("IP1 should be allowed")
	}
	now2 := now.Add(1 * time.Minute)
	l.now = func() time.Time { return now2 }
	if _, ok := l.Take("10.0.0.2"); !ok {
		t.Fatal("IP2 should be allowed")
	}

	// Adding IP3 at a later time should evict IP1 (oldest lastRefill).
	now3 := now.Add(2 * time.Minute)
	l.now = func() time.Time { return now3 }
	if _, ok := l.Take("10.0.0.3"); !ok {
		t.Fatal("IP3 should be allowed (evicted oldest entry)")
	}

	// IP1 was evicted; re-access creates a fresh bucket with full tokens.
	// Take consumes one, so it should succeed.
	l.now = func() time.Time { return now3 }
	if _, ok := l.Take("10.0.0.1"); !ok {
		t.Fatal("IP1 should have been evicted and get a fresh bucket")
	}

	// Verify we cannot have more than MaxEntries entries.
	l.mu.Lock()
	entryCount := len(l.entries)
	l.mu.Unlock()
	if entryCount > 2 {
		t.Fatalf("entry count = %d, want <= 2", entryCount)
	}
}

func TestLoginRateLimiter_CleanupRemovesStaleEntries(t *testing.T) {
	now := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	l := NewLoginRateLimiter(LoginRateLimitConfig{
		Capacity:     5,
		RefillPeriod: 10 * time.Minute,
		StaleTTL:     15 * time.Minute,
		MaxEntries:   10,
	})
	l.now = func() time.Time { return now }

	// Add two entries.
	l.Take("10.0.0.1")
	l.Take("10.0.0.2")

	// Advance past StaleTTL.
	later := now.Add(20 * time.Minute)
	l.now = func() time.Time { return later }

	l.Cleanup()

	// Both should have been removed; fresh Take succeeds.
	if _, ok := l.Take("10.0.0.1"); !ok {
		t.Fatal("IP1 should have been cleaned up and get fresh tokens")
	}
	if _, ok := l.Take("10.0.0.2"); !ok {
		t.Fatal("IP2 should have been cleaned up and get fresh tokens")
	}

	// Non-stale entries should survive cleanup.
	now3 := later.Add(1 * time.Minute)
	l.now = func() time.Time { return now3 }
	l.Take("10.0.0.3")

	now4 := now3.Add(5 * time.Minute) // still within StaleTTL from the Take
	l.now = func() time.Time { return now4 }
	l.Cleanup()

	// IP3 was accessed at now3, still fresh.
	l.mu.Lock()
	_, exists := l.entries["10.0.0.3"]
	l.mu.Unlock()
	if !exists {
		t.Fatal("IP3 should still exist after cleanup (lastRefill is recent)")
	}
}

func TestLoginRateLimiter_CleanupLoop(t *testing.T) {
	l := NewLoginRateLimiter(LoginRateLimitConfig{
		Capacity:        5,
		RefillPeriod:    10 * time.Minute,
		StaleTTL:        1 * time.Millisecond,
		CleanupInterval: 10 * time.Millisecond,
		MaxEntries:      100,
	})
	l.now = time.Now

	// Add an entry.
	l.Take("10.0.0.1")

	// Start cleanup loop.
	l.StartCleanupLoop()

	// Wait long enough for at least one cleanup tick.
	time.Sleep(30 * time.Millisecond)

	// The entry should have been cleaned up (StaleTTL = 1ms).
	l.mu.Lock()
	_, exists := l.entries["10.0.0.1"]
	l.mu.Unlock()
	if exists {
		t.Fatal("entry should have been cleaned up by background loop")
	}

	l.StopCleanupLoop()
}

func TestLoginRateLimiter_CleanupLoopIdempotent(t *testing.T) {
	l := NewLoginRateLimiter(LoginRateLimitConfig{MaxEntries: 10})

	l.StartCleanupLoop()
	l.StartCleanupLoop() // second start should be a no-op
	l.StopCleanupLoop()
	l.StopCleanupLoop() // second stop should be safe (no panic)
}
