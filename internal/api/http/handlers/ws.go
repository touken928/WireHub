package handlers

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/api/http/auth"
)

func StatusWS(s *Server, c *gin.Context) {
	token := strings.TrimSpace(c.Query("token"))
	if token == "" {
		header := c.GetHeader("Authorization")
		if parts := strings.SplitN(header, " ", 2); len(parts) == 2 && strings.EqualFold(parts[0], "Bearer") {
			token = parts[1]
		}
	}
	if token == "" {
		c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "missing token"})
		return
	}
	authSvc, ok := c.Get("auth")
	if !ok {
		c.AbortWithStatus(http.StatusInternalServerError)
		return
	}
	if _, err := authSvc.(*auth.Service).ParseToken(token); err != nil {
		c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "invalid token"})
		return
	}
	if s.StatusWS == nil {
		c.AbortWithStatus(http.StatusServiceUnavailable)
		return
	}
	s.StatusWS.Serve(c.Writer, c.Request)
}
