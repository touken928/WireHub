package server

import (
	"strconv"

	"github.com/gin-gonic/gin"
)

func parseID(c *gin.Context) (uint, error) {
	id, err := strconv.ParseUint(c.Param("id"), 10, 64)
	return uint(id), err
}
