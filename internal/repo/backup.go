package repo

import (
	"database/sql"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"


)

const sqliteHeader = "SQLite format 3\x00"

var wireHubTables = []string{
	"admins", "settings", "peer_groups", "group_links", "peers", "dns_records",
}

// ValidateWireHubDatabase checks that path is a configured WireHub SQLite database.
func ValidateWireHubDatabase(path string) error {
	st, err := os.Stat(path)
	if err != nil {
		return fmt.Errorf("database file: %w", err)
	}
	if st.Size() < 512 {
		return fmt.Errorf("database file is too small")
	}
	if st.Size() > 128<<20 {
		return fmt.Errorf("database file exceeds 128MB limit")
	}

	head := make([]byte, len(sqliteHeader))
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	if _, err := io.ReadFull(f, head); err != nil {
		_ = f.Close()
		return fmt.Errorf("read database header: %w", err)
	}
	_ = f.Close()
	if string(head) != sqliteHeader {
		return fmt.Errorf("not a SQLite database file")
	}

	db, err := sql.Open("sqlite", path)
	if err != nil {
		return fmt.Errorf("open database: %w", err)
	}
	defer db.Close()

	for _, table := range wireHubTables {
		var name string
		err := db.QueryRow(
			`SELECT name FROM sqlite_master WHERE type='table' AND name=?`,
			table,
		).Scan(&name)
		if err != nil {
			return fmt.Errorf("missing table %q", table)
		}
	}

	var adminCount int
	if err := db.QueryRow(`SELECT COUNT(*) FROM admins`).Scan(&adminCount); err != nil || adminCount == 0 {
		return fmt.Errorf("database has no admin account")
	}

	var settingsCount int
	if err := db.QueryRow(`SELECT COUNT(*) FROM settings`).Scan(&settingsCount); err != nil || settingsCount == 0 {
		return fmt.Errorf("database has no hub settings")
	}

	var endpoint string
	if err := db.QueryRow(`SELECT endpoint FROM settings LIMIT 1`).Scan(&endpoint); err != nil {
		return fmt.Errorf("read settings: %w", err)
	}
	if strings.TrimSpace(endpoint) == "" {
		return fmt.Errorf("hub endpoint is not configured in database")
	}

	return nil
}

func (s *Store) DatabasePath() string {
	return s.dbPath
}

func (s *Store) closeDB() error {
	if s.db == nil {
		return nil
	}
	sqlDB, err := s.db.DB()
	if err != nil {
		return err
	}
	if err := sqlDB.Close(); err != nil {
		return err
	}
	s.db = nil
	return nil
}

// ExportDatabase writes a consistent snapshot of wirehub.db.
func (s *Store) ExportDatabase(w io.Writer) error {
	if err := s.db.Exec("PRAGMA wal_checkpoint(FULL)").Error; err != nil {
		return fmt.Errorf("checkpoint: %w", err)
	}
	f, err := os.Open(s.dbPath)
	if err != nil {
		return fmt.Errorf("open database file: %w", err)
	}
	defer f.Close()
	if _, err := io.Copy(w, f); err != nil {
		return fmt.Errorf("copy database: %w", err)
	}
	return nil
}

// ImportDatabase replaces the live database with a validated backup file.
func (s *Store) ImportDatabase(srcPath string) error {
	if err := ValidateWireHubDatabase(srcPath); err != nil {
		return err
	}
	if err := s.closeDB(); err != nil {
		return err
	}
	if err := copyFileAtomic(srcPath, s.dbPath); err != nil {
		_ = s.openDB()
		return err
	}
	if err := s.openDB(); err != nil {
		return err
	}
	configured, err := s.IsConfigured()
	if err != nil {
		return err
	}
	if !configured {
		return fmt.Errorf("imported database is not a configured WireHub hub")
	}
	return nil
}

func copyFileAtomic(src, dst string) error {
	data, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	dir := filepath.Dir(dst)
	tmp, err := os.CreateTemp(dir, "wirehub-import-*.db")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		_ = os.Remove(tmpPath)
		return err
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmpPath)
		return err
	}
	if err := os.Rename(tmpPath, dst); err != nil {
		_ = os.Remove(tmpPath)
		return err
	}
	_ = os.Remove(dst + "-wal")
	_ = os.Remove(dst + "-shm")
	return nil
}
