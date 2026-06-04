package httputil

import (
	"math"
	"sync"
	"time"
)

const (
	loginRateCapacity     = 5
	loginRateRefillPeriod = 10 * time.Minute
)

// LoginRateLimitConfig controls the login token bucket per client IP.
type LoginRateLimitConfig struct {
	Capacity     float64
	RefillPeriod time.Duration
}

func (c LoginRateLimitConfig) normalized() LoginRateLimitConfig {
	out := c
	if out.Capacity <= 0 {
		out.Capacity = loginRateCapacity
	}
	if out.RefillPeriod <= 0 {
		out.RefillPeriod = loginRateRefillPeriod
	}
	return out
}

// LoginRateLimiter tracks login attempt tokens per IP in memory.
type LoginRateLimiter struct {
	mu      sync.Mutex
	cfg     LoginRateLimitConfig
	entries map[string]*loginRateEntry
	now     func() time.Time
}

type loginRateEntry struct {
	tokens     float64
	lastRefill time.Time
}

// NewLoginRateLimiter returns a limiter with the given config.
func NewLoginRateLimiter(cfg LoginRateLimitConfig) *LoginRateLimiter {
	return &LoginRateLimiter{
		cfg:     cfg.normalized(),
		entries: make(map[string]*loginRateEntry),
		now:     time.Now,
	}
}

// DefaultLoginRateLimiter allows 5 login attempts per 10 minutes.
func DefaultLoginRateLimiter() *LoginRateLimiter {
	return NewLoginRateLimiter(LoginRateLimitConfig{})
}

// Take consumes one token for a login attempt from ip.
func (l *LoginRateLimiter) Take(ip string) (retryAfter time.Duration, ok bool) {
	if ip == "" {
		return 0, true
	}
	now := l.now()
	l.mu.Lock()
	defer l.mu.Unlock()

	e := l.entryLocked(ip, now)
	l.refillLocked(e, now)
	if e.tokens >= 1 {
		e.tokens -= 1
		return 0, true
	}
	rate := l.refillRate()
	if rate <= 0 {
		return l.cfg.RefillPeriod, false
	}
	need := 1 - e.tokens
	secs := need / rate
	if secs < 0 {
		secs = 0
	}
	return time.Duration(math.Ceil(secs * float64(time.Second))), false
}

// RecordLoginSuccess clears token state for ip after a successful login.
func (l *LoginRateLimiter) RecordLoginSuccess(ip string) {
	if ip == "" {
		return
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	delete(l.entries, ip)
}

func (l *LoginRateLimiter) entryLocked(ip string, now time.Time) *loginRateEntry {
	e, ok := l.entries[ip]
	if !ok {
		e = &loginRateEntry{
			tokens:     l.cfg.Capacity,
			lastRefill: now,
		}
		l.entries[ip] = e
	}
	return e
}

func (l *LoginRateLimiter) refillLocked(e *loginRateEntry, now time.Time) {
	if e.lastRefill.IsZero() {
		e.tokens = l.cfg.Capacity
		e.lastRefill = now
		return
	}
	elapsed := now.Sub(e.lastRefill)
	if elapsed <= 0 {
		return
	}
	e.tokens = math.Min(l.cfg.Capacity, e.tokens+elapsed.Seconds()*l.refillRate())
	e.lastRefill = now
}

func (l *LoginRateLimiter) refillRate() float64 {
	sec := l.cfg.RefillPeriod.Seconds()
	if sec <= 0 {
		return 0
	}
	return l.cfg.Capacity / sec
}
