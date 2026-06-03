package server

import (
	"math"
	"net/http"
	"strconv"

	"github.com/gin-gonic/gin"
)

func parseID(c *gin.Context) (uint, error) {
	id, err := strconv.ParseUint(c.Param("id"), 10, 64)
	return uint(id), err
}

func clientIP(c *gin.Context) string {
	return c.ClientIP()
}

func (s *Server) rejectLoginRateLimit(c *gin.Context, ip string) bool {
	if s.loginLimiter == nil {
		return false
	}
	retryAfter, ok := s.loginLimiter.Take(ip)
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
