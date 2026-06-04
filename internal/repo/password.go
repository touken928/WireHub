package repo

import (
	"fmt"

	"golang.org/x/crypto/bcrypt"
)

const minPasswordLen = 8

// ValidatePassword checks admin password policy before hashing.
func ValidatePassword(plain string) error {
	if len(plain) < minPasswordLen {
		return fmt.Errorf("password must be at least %d characters", minPasswordLen)
	}
	return nil
}

// HashPassword bcrypt-hashes a plaintext admin password.
func HashPassword(plain string) (string, error) {
	if err := ValidatePassword(plain); err != nil {
		return "", err
	}
	hash, err := bcrypt.GenerateFromPassword([]byte(plain), bcrypt.DefaultCost)
	if err != nil {
		return "", err
	}
	return string(hash), nil
}

// VerifyPassword compares a bcrypt hash with plaintext.
func VerifyPassword(hash, plain string) error {
	return bcrypt.CompareHashAndPassword([]byte(hash), []byte(plain))
}
