package store

import (
	"fmt"
	"log"
	"net"
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
	return s.db.AutoMigrate(&Admin{}, &Settings{}, &PeerGroup{}, &GroupLink{}, &Peer{}, &DNSRecord{})
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

func (s *Store) ListPeers() ([]Peer, error) {
	var peers []Peer
	err := s.db.Order("created_at desc").Find(&peers).Error
	return peers, err
}

func (s *Store) GetPeer(id uint) (*Peer, error) {
	var peer Peer
	if err := s.db.First(&peer, id).Error; err != nil {
		return nil, err
	}
	return &peer, nil
}

func (s *Store) CreatePeer(peer *Peer) error {
	return s.db.Create(peer).Error
}

func (s *Store) UpdatePeer(peer *Peer) error {
	return s.db.Save(peer).Error
}

func (s *Store) DeletePeer(id uint) error {
	return s.db.Delete(&Peer{}, id).Error
}

func (s *Store) AllocateIP(subnet, hubIP string) (string, error) {
	_, ipNet, err := net.ParseCIDR(subnet)
	if err != nil {
		return "", err
	}

	var peers []Peer
	if err := s.db.Find(&peers).Error; err != nil {
		return "", err
	}

	used := map[string]bool{hubIP: true}
	for _, p := range peers {
		used[p.WGIP] = true
	}

	base := ipNet.IP.To4()
	if base == nil {
		return "", fmt.Errorf("only IPv4 subnets supported")
	}

	mask, _ := ipNet.Mask.Size()
	for i := 2; i < (1 << (32 - mask)); i++ {
		ip := make(net.IP, 4)
		copy(ip, base)
		ip[3] = base[3] + byte(i)
		candidate := ip.String()
		if !used[candidate] {
			return candidate, nil
		}
	}
	return "", fmt.Errorf("no available IP in subnet")
}

func (s *Store) ListDNSRecords() ([]DNSRecord, error) {
	var records []DNSRecord
	err := s.db.Order("hostname asc").Find(&records).Error
	return records, err
}

func (s *Store) GetDNSRecord(id uint) (*DNSRecord, error) {
	var record DNSRecord
	if err := s.db.First(&record, id).Error; err != nil {
		return nil, err
	}
	return &record, nil
}

func (s *Store) CreateDNSRecord(record *DNSRecord) error {
	return s.db.Create(record).Error
}

func (s *Store) UpdateDNSRecord(record *DNSRecord) error {
	return s.db.Save(record).Error
}

func (s *Store) DeleteDNSRecord(id uint) error {
	return s.db.Delete(&DNSRecord{}, id).Error
}

func (s *Store) DeleteDNSByPeerID(peerID uint) error {
	return s.db.Where("peer_id = ? AND manual = ?", peerID, false).Delete(&DNSRecord{}).Error
}

func (s *Store) ResolveDNS(hostname string) (string, bool) {
	var records []DNSRecord
	if err := s.db.Where("hostname = ?", hostname).Limit(1).Find(&records).Error; err != nil {
		return "", false
	}
	if len(records) == 0 {
		return "", false
	}
	return records[0].IP, true
}

func (s *Store) UpdatePeerStats(id uint, lastHandshake, rx, tx int64) error {
	return s.db.Model(&Peer{}).Where("id = ?", id).Updates(map[string]interface{}{
		"last_handshake": lastHandshake,
		"rx_bytes":       rx,
		"tx_bytes":       tx,
	}).Error
}
