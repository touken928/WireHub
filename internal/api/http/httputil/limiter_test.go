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
}
