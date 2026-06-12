package repo

import (
	"errors"
	"fmt"
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

// AllocateIP picks the next free host address in the VPN subnet (hub, map VIPs, and DNS reserved).
func (s *Store) AllocateIP(subnet, hubIP, dnsIP string) (string, error) {
	ip, err := s.allocateSubnetIP(subnet, hubIP, dnsIP)
	if errors.Is(err, errSubnetIPUnavailable) {
		return "", fmt.Errorf("no available IP in subnet")
	}
	return ip, err
}

func (s *Store) CreateDNSRecord(record *DNSRecord) error {
	return s.db.Create(record).Error
}

func (s *Store) DeleteDNSByPeerID(peerID uint) error {
	return s.db.Where("peer_id = ? AND manual = ?", peerID, false).Delete(&DNSRecord{}).Error
}

func (s *Store) UpdatePeerStats(id uint, lastHandshake, rx, tx int64) error {
	return s.db.Model(&Peer{}).Where("id = ?", id).Updates(map[string]interface{}{
		"last_handshake": lastHandshake,
		"rx_bytes":       rx,
		"tx_bytes":       tx,
	}).Error
}
