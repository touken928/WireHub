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

	r.Use(corsMiddleware, noCacheAssetsMiddleware)

	if assetsFS, err := fs.Sub(webFS, "assets"); err == nil {
		r.StaticFS("/assets", http.FS(assetsFS))
	}

	for _, name := range []string{"favicon.svg", "icons.svg"} {
		if _, err := fs.Stat(webFS, name); err == nil {
			r.GET("/"+name, serveFile(webFS, name))
		}
	}

	r.GET("/", serveIndexHTML(webFS))

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
		if isLikelyAssetPath(path) {
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

		serveIndexHTML(webFS)(c)
	})

	return nil
}

func noCacheAssetsMiddleware(c *gin.Context) {
	if strings.HasPrefix(c.Request.URL.Path, "/assets/") {
		c.Header("Cache-Control", "no-cache, no-store, must-revalidate")
	}
	c.Next()
}

func corsMiddleware(c *gin.Context) {
	origin := c.GetHeader("Origin")
	if origin != "" {
		c.Header("Access-Control-Allow-Origin", origin)
		c.Header("Vary", "Origin")
		c.Header("Access-Control-Allow-Credentials", "true")
	}
	if c.Request.Method == http.MethodOptions {
		c.Header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
		c.Header("Access-Control-Allow-Headers", "Authorization, Content-Type")
		c.AbortWithStatus(http.StatusNoContent)
		return
	}
	c.Next()
}

func isLikelyAssetPath(path string) bool {
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".svg", ".woff", ".woff2", ".ttf", ".map":
		return true
	default:
		return false
	}
}

func assetContentType(name string) string {
	switch strings.ToLower(filepath.Ext(name)) {
	case ".js", ".mjs":
		return "text/javascript; charset=utf-8"
	case ".css":
		return "text/css; charset=utf-8"
	case ".svg":
		return "image/svg+xml"
	case ".json":
		return "application/json; charset=utf-8"
	case ".wasm":
		return "application/wasm"
	default:
		ctype := mime.TypeByExtension(filepath.Ext(name))
		if ctype == "" {
			return "application/octet-stream"
		}
		return ctype
	}
}

func serveIndexHTML(webFS fs.FS) gin.HandlerFunc {
	return func(c *gin.Context) {
		data, err := fs.ReadFile(webFS, "index.html")
		if err != nil {
			c.Status(http.StatusNotFound)
			return
		}
		c.Header("Cache-Control", "no-cache, no-store, must-revalidate")
		c.Data(http.StatusOK, "text/html; charset=utf-8", data)
	}
}

func serveFile(webFS fs.FS, name string) gin.HandlerFunc {
	return func(c *gin.Context) {
		data, err := fs.ReadFile(webFS, name)
		if err != nil {
			c.Status(http.StatusNotFound)
			return
		}
		c.Data(http.StatusOK, assetContentType(name), data)
	}
}
