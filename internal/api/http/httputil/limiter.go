package httputil

import (
	"math"
	"sync"
	"time"
)

const (
	loginRateCapacity          = 5
	loginRateRefillPeriod      = 10 * time.Minute
	loginRateMaxEntries        = 10000
	loginRateCleanupInterval   = 5 * time.Minute
	loginRateStaleTTL          = 20 * time.Minute // 2× refill period
)

// LoginRateLimitConfig controls the login token bucket per client IP.
type LoginRateLimitConfig struct {
	Capacity        float64
	RefillPeriod    time.Duration
	MaxEntries      int           // max tracked IPs before evicting oldest
	CleanupInterval time.Duration // how often the background cleanup runs
	StaleTTL        time.Duration // entries idle longer than this are removed by cleanup
}

func (c LoginRateLimitConfig) normalized() LoginRateLimitConfig {
	out := c
	if out.Capacity <= 0 {
		out.Capacity = loginRateCapacity
	}
	if out.RefillPeriod <= 0 {
		out.RefillPeriod = loginRateRefillPeriod
	}
	if out.MaxEntries <= 0 {
		out.MaxEntries = loginRateMaxEntries
	}
	if out.CleanupInterval <= 0 {
		out.CleanupInterval = loginRateCleanupInterval
	}
	if out.StaleTTL <= 0 {
		out.StaleTTL = loginRateStaleTTL
	}
	return out
}

// LoginRateLimiter tracks login attempt tokens per IP in memory.
// The map is bounded by MaxEntries; idle entries are removed by Cleanup
// or via the optional background cleanup loop.
type LoginRateLimiter struct {
	mu          sync.Mutex
	cfg         LoginRateLimitConfig
	entries     map[string]*loginRateEntry
	now         func() time.Time
	stopCleanup chan struct{}
	cleanupWg   sync.WaitGroup
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

// Cleanup removes entries that have been idle longer than StaleTTL.
func (l *LoginRateLimiter) Cleanup() {
	l.mu.Lock()
	defer l.mu.Unlock()
	cutoff := l.now().Add(-l.cfg.StaleTTL)
	for k, v := range l.entries {
		if v.lastRefill.Before(cutoff) {
			delete(l.entries, k)
		}
	}
}

// StartCleanupLoop begins a background goroutine that runs Cleanup periodically.
func (l *LoginRateLimiter) StartCleanupLoop() {
	l.mu.Lock()
	if l.stopCleanup != nil {
		l.mu.Unlock()
		return // already running
	}
	l.stopCleanup = make(chan struct{})
	l.cleanupWg.Add(1)
	stopCh := l.stopCleanup // capture so the goroutine is not affected by future field mutation
	l.mu.Unlock()

	go func() {
		defer l.cleanupWg.Done()
		ticker := time.NewTicker(l.cfg.CleanupInterval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				l.Cleanup()
			case <-stopCh:
				return
			}
		}
	}()
}

// StopCleanupLoop stops the background cleanup goroutine and waits for it to exit.
func (l *LoginRateLimiter) StopCleanupLoop() {
	l.mu.Lock()
	ch := l.stopCleanup
	l.stopCleanup = nil
	l.mu.Unlock()
	if ch != nil {
		close(ch)
		l.cleanupWg.Wait()
	}
}

func (l *LoginRateLimiter) entryLocked(ip string, now time.Time) *loginRateEntry {
	e, ok := l.entries[ip]
	if !ok {
		// Evict oldest entry when at capacity to bound memory growth.
		if len(l.entries) >= l.cfg.MaxEntries {
			var oldestKey string
			var oldestTime time.Time
			for k, v := range l.entries {
				if oldestKey == "" || v.lastRefill.Before(oldestTime) {
					oldestKey = k
					oldestTime = v.lastRefill
				}
			}
			delete(l.entries, oldestKey)
		}
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
