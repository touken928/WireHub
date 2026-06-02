package static

import (
	"embed"
	"io/fs"
	"mime"
	"net/http"
	"path/filepath"
	"strings"

	"github.com/gin-gonic/gin"
)

//go:embed all:dist
var embedded embed.FS

// Dist returns the embedded frontend build output (dist/).
func Dist() (fs.FS, error) {
	return fs.Sub(embedded, "dist")
}

// Mount serves the embedded SPA on the Gin router.
// Register API routes before calling Mount.
func Mount(r *gin.Engine) error {
	webFS, err := Dist()
	if err != nil {
		return err
	}

	if assetsFS, err := fs.Sub(webFS, "assets"); err == nil {
		r.StaticFS("/assets", http.FS(assetsFS))
	}

	for _, name := range []string{"favicon.svg", "icons.svg"} {
		if _, err := fs.Stat(webFS, name); err == nil {
			r.GET("/"+name, serveFile(webFS, name))
		}
	}

	r.GET("/", serveFile(webFS, "index.html"))

	r.NoRoute(func(c *gin.Context) {
		path := c.Request.URL.Path
		if strings.HasPrefix(path, "/api") {
			c.JSON(http.StatusNotFound, gin.H{"error": "not found"})
			return
		}
		if strings.HasPrefix(path, "/assets") {
			c.Status(http.StatusNotFound)
			return
		}

		clean := strings.TrimPrefix(path, "/")
		if clean != "" {
			if _, err := fs.Stat(webFS, clean); err == nil {
				serveFile(webFS, clean)(c)
				return
			}
		}

		serveFile(webFS, "index.html")(c)
	})

	return nil
}

func serveFile(webFS fs.FS, name string) gin.HandlerFunc {
	return func(c *gin.Context) {
		data, err := fs.ReadFile(webFS, name)
		if err != nil {
			c.Status(http.StatusNotFound)
			return
		}
		ctype := mime.TypeByExtension(filepath.Ext(name))
		if ctype == "" {
			ctype = "application/octet-stream"
		}
		c.Data(http.StatusOK, ctype, data)
	}
}
