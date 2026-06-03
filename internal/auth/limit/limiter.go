// Package limit provides in-memory token-bucket rate limiting for login attempts.
package limit

import (
	"math"
	"sync"
	"time"
)

const (
	DefaultCapacity     = 5
	DefaultRefillPeriod = 10 * time.Minute
)

// Config controls the login token bucket per client IP.
// Capacity is the maximum tokens (burst). RefillPeriod is the time to refill from empty to full.
type Config struct {
	Capacity     float64
	RefillPeriod time.Duration
}

func (c Config) normalized() Config {
	out := c
	if out.Capacity <= 0 {
		out.Capacity = DefaultCapacity
	}
	if out.RefillPeriod <= 0 {
		out.RefillPeriod = DefaultRefillPeriod
	}
	return out
}

// Limiter tracks login attempt tokens per IP in memory.
type Limiter struct {
	mu      sync.Mutex
	cfg     Config
	entries map[string]*entry
	now     func() time.Time
}

type entry struct {
	tokens     float64
	lastRefill time.Time
}

// New returns a limiter with the given config. Zero values use package defaults.
func New(cfg Config) *Limiter {
	return &Limiter{
		cfg:     cfg.normalized(),
		entries: make(map[string]*entry),
		now:     time.Now,
	}
}

// DefaultLimiter allows 5 login attempts per 10 minutes (token bucket).
func DefaultLimiter() *Limiter {
	return New(Config{})
}

// Take consumes one token for a login attempt from ip.
// When false, retryAfter is an estimate until the next token is available.
func (l *Limiter) Take(ip string) (retryAfter time.Duration, ok bool) {
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

// RecordSuccess clears token state for ip after a successful login.
func (l *Limiter) RecordSuccess(ip string) {
	if ip == "" {
		return
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	delete(l.entries, ip)
}

func (l *Limiter) entryLocked(ip string, now time.Time) *entry {
	e, ok := l.entries[ip]
	if !ok {
		e = &entry{
			tokens:     l.cfg.Capacity,
			lastRefill: now,
		}
		l.entries[ip] = e
	}
	return e
}

func (l *Limiter) refillLocked(e *entry, now time.Time) {
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

func (l *Limiter) refillRate() float64 {
	sec := l.cfg.RefillPeriod.Seconds()
	if sec <= 0 {
		return 0
	}
	return l.cfg.Capacity / sec
}
