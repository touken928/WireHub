package httputil

import (
	"math"
	"net/http"
	"strconv"

	"github.com/gin-gonic/gin"
)

// RejectLoginRateLimit returns true when the client is rate-limited (response already written).
func RejectLoginRateLimit(c *gin.Context, lim *LoginRateLimiter, ip string) bool {
	if lim == nil {
		return false
	}
	retryAfter, ok := lim.Take(ip)
	if ok {
		return false
	}
	seconds := int(math.Ceil(retryAfter.Seconds()))
	if seconds < 1 {
		seconds = 1
	}
	c.Header("Retry-After", strconv.Itoa(seconds))
	c.JSON(http.StatusTooManyRequests, gin.H{"error": "too many login attempts"})
	return true
}
