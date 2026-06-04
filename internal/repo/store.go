package repo

import (
	"fmt"
	"log"
	"os"
	"path/filepath"

	"github.com/touken928/wirehub/internal/config"
	"gorm.io/gorm"
	"gorm.io/gorm/logger"

	gormsqlite "github.com/glebarez/sqlite"
)

type Store struct {
	db     *gorm.DB
	dbPath string
}

func New(cfg *config.RuntimeConfig) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(cfg.DatabasePath), 0o755); err != nil {
		return nil, fmt.Errorf("create data dir: %w", err)
	}

	s := &Store{dbPath: cfg.DatabasePath}
	if err := s.openDB(); err != nil {
		return nil, err
	}
	return s, nil
}

func (s *Store) openDB() error {
	db, err := gorm.Open(gormsqlite.Open(s.dbPath), &gorm.Config{
		Logger: logger.New(log.New(os.Stdout, "\r\n", log.LstdFlags), logger.Config{
			LogLevel:                  logger.Warn,
			IgnoreRecordNotFoundError: true,
		}),
	})
	if err != nil {
		return fmt.Errorf("open database: %w", err)
	}
	s.db = db
	if err := s.migrate(); err != nil {
		return err
	}
	return s.MigrateGroups()
}

func (s *Store) DB() *gorm.DB { return s.db }

func (s *Store) migrate() error {
	return s.db.AutoMigrate(&Admin{}, &Settings{}, &PeerGroup{}, &GroupLink{}, &Peer{}, &DNSRecord{}, &PortForward{}, &ServiceMap{}, &MapGroupAllow{})
}

func (s *Store) GetSettings() (*Settings, error) {
	var settings Settings
	if err := s.db.First(&settings).Error; err != nil {
		return nil, err
	}
	return &settings, nil
}

func (s *Store) UpdateSettings(settings *Settings) error {
	return s.db.Save(settings).Error
}

func (s *Store) GetAdminByUsername(username string) (*Admin, error) {
	var admin Admin
	if err := s.db.Where("username = ?", username).First(&admin).Error; err != nil {
		return nil, err
	}
	return &admin, nil
}
