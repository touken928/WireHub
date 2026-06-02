// Package password provides bcrypt helpers shared by repo and auth without import cycles.
package password

import (
	"fmt"

	"golang.org/x/crypto/bcrypt"
)

const minLen = 8

func Validate(plain string) error {
	if len(plain) < minLen {
		return fmt.Errorf("password must be at least %d characters", minLen)
	}
	return nil
}

func Hash(plain string) (string, error) {
	if err := Validate(plain); err != nil {
		return "", err
	}
	hash, err := bcrypt.GenerateFromPassword([]byte(plain), bcrypt.DefaultCost)
	if err != nil {
		return "", err
	}
	return string(hash), nil
}

func Verify(hash, plain string) error {
	return bcrypt.CompareHashAndPassword([]byte(hash), []byte(plain))
}
