package api

import (
	"strconv"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/wg"
)

func parseID(c *gin.Context) (uint, error) {
	id, err := strconv.ParseUint(c.Param("id"), 10, 64)
	return uint(id), err
}

func wgGenerateKeyPair() (string, string, error) {
	return wg.GenerateKeyPair()
}
