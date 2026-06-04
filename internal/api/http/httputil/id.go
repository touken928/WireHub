package httputil

import (
	"strconv"

	"github.com/gin-gonic/gin"
)

// ParseID reads a uint id from a Gin path parameter.
func ParseID(c *gin.Context) (uint, error) {
	id, err := strconv.ParseUint(c.Param("id"), 10, 64)
	return uint(id), err
}

// ClientIP returns the request client address.
func ClientIP(c *gin.Context) string {
	return c.ClientIP()
}
