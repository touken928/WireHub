package repo

import (
	"fmt"
	"net"
)

// ListPeers returns all peers ordered by creation time (newest first).
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

// AllocateIP picks the next free host address in the VPN subnet (hub IP excluded).
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

// ResolveDNS looks up a manual or peer-backed hostname in the local DNS table.
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
