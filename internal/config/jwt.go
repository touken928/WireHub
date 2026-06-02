package config

import (
	"crypto/rand"
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"
)

func loadOrCreateJWTSecret(dataDir string) (string, error) {
	path := filepath.Join(dataDir, ".jwt_secret")
	if data, err := os.ReadFile(path); err == nil {
		if secret := string(data); secret != "" {
			return secret, nil
		}
	}

	secret, err := generateJWTSecret()
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(path, []byte(secret), 0o600); err != nil {
		return "", fmt.Errorf("write jwt secret: %w", err)
	}
	return secret, nil
}

func generateJWTSecret() (string, error) {
	buf := make([]byte, 32)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("generate jwt secret: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(buf), nil
}
