package auth

import (
	"errors"
	"time"

	"github.com/golang-jwt/jwt/v5"
	passwd "github.com/touken928/wirehub/internal/password"
	"github.com/touken928/wirehub/internal/repo"
)

type Claims struct {
	AdminID  uint   `json:"admin_id"`
	Username string `json:"username"`
	jwt.RegisteredClaims
}

type Service struct {
	secret string
	store  *repo.Store
}

func NewService(secret string, st *repo.Store) *Service {
	return &Service{secret: secret, store: st}
}

func (s *Service) Login(username, password string) (string, error) {
	admin, err := s.store.GetAdminByUsername(username)
	if err != nil {
		return "", errors.New("invalid credentials")
	}
	if err := passwd.Verify(admin.PasswordHash, password); err != nil {
		return "", errors.New("invalid credentials")
	}
	return s.issueToken(admin)
}

func (s *Service) issueToken(admin *repo.Admin) (string, error) {
	claims := Claims{
		AdminID:  admin.ID,
		Username: admin.Username,
		RegisteredClaims: jwt.RegisteredClaims{
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(24 * time.Hour)),
			IssuedAt:  jwt.NewNumericDate(time.Now()),
		},
	}
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	return token.SignedString([]byte(s.secret))
}

func (s *Service) ParseToken(tokenStr string) (*Claims, error) {
	token, err := jwt.ParseWithClaims(tokenStr, &Claims{}, func(t *jwt.Token) (interface{}, error) {
		return []byte(s.secret), nil
	})
	if err != nil {
		return nil, err
	}
	claims, ok := token.Claims.(*Claims)
	if !ok || !token.Valid {
		return nil, errors.New("invalid token")
	}
	return claims, nil
}
