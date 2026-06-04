package repo

import (
	"errors"
	"fmt"
	"strings"

	"github.com/touken928/wirehub/internal/domain/map"
	"gorm.io/gorm"
)

var ErrMapSlugConflict = errors.New("map slug already in use")
var ErrMapIPUnavailable = errors.New("no map virtual ip available in subnet")

type ServiceMap struct {
	ID         uint   `gorm:"primaryKey" json:"id"`
	Name       string `json:"name"`
	Slug       string `gorm:"uniqueIndex;not null" json:"slug"`
	TargetHost string `gorm:"not null" json:"target_host"`
	VirtualIP  string `gorm:"uniqueIndex;not null" json:"virtual_ip"`
}

func (ServiceMap) TableName() string { return "service_relays" }

type MapGroupAllow struct {
	MapID   uint `gorm:"primaryKey;column:relay_id" json:"map_id"`
	GroupID uint `gorm:"primaryKey" json:"group_id"`
}

func (MapGroupAllow) TableName() string { return "relay_group_allows" }

type MapInput struct {
	Name          string
	Slug          string
	TargetHost    string
	AllowedGroups []uint
}

type MapDetail struct {
	ServiceMap
	TargetDisplay string `json:"target_display"`
	AllowedGroups []uint `json:"allowed_group_ids"`
}

func (s *Store) ListServiceMaps() ([]ServiceMap, error) {
	var maps []ServiceMap
	err := s.db.Order("slug asc").Find(&maps).Error
	return maps, err
}

func (s *Store) GetServiceMap(id uint) (*ServiceMap, error) {
	var entry ServiceMap
	if err := s.db.First(&entry, id).Error; err != nil {
		return nil, err
	}
	return &entry, nil
}

func (s *Store) GetServiceMapBySlug(slug string) (*ServiceMap, error) {
	var entry ServiceMap
	if err := s.db.Where("slug = ?", slug).First(&entry).Error; err != nil {
		return nil, err
	}
	return &entry, nil
}

func (s *Store) ListMapGroupIDs(mapID uint) ([]uint, error) {
	var rows []MapGroupAllow
	if err := s.db.Where("relay_id = ?", mapID).Find(&rows).Error; err != nil {
		return nil, err
	}
	out := make([]uint, len(rows))
	for i, r := range rows {
		out[i] = r.GroupID
	}
	return out, nil
}

func (s *Store) ListMapDetails() ([]MapDetail, error) {
	maps, err := s.ListServiceMaps()
	if err != nil {
		return nil, err
	}
	out := make([]MapDetail, len(maps))
	for i, r := range maps {
		groups, err := s.ListMapGroupIDs(r.ID)
		if err != nil {
			return nil, err
		}
		out[i] = MapDetail{
			ServiceMap:    r,
			TargetDisplay: r.TargetHost,
			AllowedGroups: groups,
		}
	}
	return out, nil
}

func (s *Store) GetMapDetail(id uint) (*MapDetail, error) {
	entry, err := s.GetServiceMap(id)
	if err != nil {
		return nil, err
	}
	groups, err := s.ListMapGroupIDs(id)
	if err != nil {
		return nil, err
	}
	return &MapDetail{
		ServiceMap:    *entry,
		TargetDisplay: entry.TargetHost,
		AllowedGroups: groups,
	}, nil
}

func (s *Store) GetPeerByWGIP(ip string) (*Peer, error) {
	var peer Peer
	if err := s.db.Where("wg_ip = ?", ip).First(&peer).Error; err != nil {
		return nil, err
	}
	return &peer, nil
}

func (s *Store) CreateServiceMap(in MapInput) (*MapDetail, error) {
	conflict, err := s.MapSlugConflictsPeer(in.Slug)
	if err != nil {
		return nil, err
	}
	if conflict {
		return nil, fmt.Errorf("slug conflicts with an existing peer name")
	}
	entry, groupIDs, err := normalizeMapInput(in)
	if err != nil {
		return nil, err
	}
	settings, err := s.GetSettings()
	if err != nil {
		return nil, err
	}
	vip, err := s.AllocateMapIP(settings.WGSubnet, settings.HubIP, settings.DNSIP)
	if err != nil {
		return nil, err
	}
	entry.VirtualIP = vip

	if err := s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Create(entry).Error; err != nil {
			if strings.Contains(err.Error(), "UNIQUE") || strings.Contains(err.Error(), "unique") {
				return ErrMapSlugConflict
			}
			return err
		}
		for _, gid := range groupIDs {
			if err := tx.Create(&MapGroupAllow{MapID: entry.ID, GroupID: gid}).Error; err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		return nil, err
	}
	return s.GetMapDetail(entry.ID)
}

func (s *Store) UpdateServiceMap(id uint, in MapInput) (*MapDetail, error) {
	existing, err := s.GetServiceMap(id)
	if err != nil {
		return nil, err
	}
	conflict, err := s.MapSlugConflictsPeer(in.Slug)
	if err != nil {
		return nil, err
	}
	if conflict {
		return nil, fmt.Errorf("slug conflicts with an existing peer name")
	}
	entry, groupIDs, err := normalizeMapInput(in)
	if err != nil {
		return nil, err
	}
	if entry.Slug != existing.Slug {
		if _, err := s.GetServiceMapBySlug(entry.Slug); err == nil {
			return nil, ErrMapSlugConflict
		} else if !errors.Is(err, gorm.ErrRecordNotFound) {
			return nil, err
		}
	}
	entry.ID = existing.ID
	entry.VirtualIP = existing.VirtualIP

	if err := s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Save(entry).Error; err != nil {
			if strings.Contains(err.Error(), "UNIQUE") || strings.Contains(err.Error(), "unique") {
				return ErrMapSlugConflict
			}
			return err
		}
		if err := tx.Where("relay_id = ?", id).Delete(&MapGroupAllow{}).Error; err != nil {
			return err
		}
		for _, gid := range groupIDs {
			if err := tx.Create(&MapGroupAllow{MapID: id, GroupID: gid}).Error; err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		return nil, err
	}
	return s.GetMapDetail(id)
}

func (s *Store) DeleteServiceMap(id uint) error {
	return s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Where("relay_id = ?", id).Delete(&MapGroupAllow{}).Error; err != nil {
			return err
		}
		return tx.Delete(&ServiceMap{}, id).Error
	})
}

func (s *Store) AllocateMapIP(subnet, hubIP, dnsIP string) (string, error) {
	ip, err := s.allocateSubnetIP(subnet, hubIP, dnsIP)
	if errors.Is(err, errSubnetIPUnavailable) {
		return "", ErrMapIPUnavailable
	}
	return ip, err
}

func normalizeMapInput(in MapInput) (*ServiceMap, []uint, error) {
	slug, err := mapdom.ValidateMapSlug(in.Slug)
	if err != nil {
		return nil, nil, err
	}
	target, err := mapdom.ValidateMapTargetHost(in.TargetHost)
	if err != nil {
		return nil, nil, err
	}
	if err := mapdom.ValidateMapGroupIDs(in.AllowedGroups); err != nil {
		return nil, nil, err
	}

	name := strings.TrimSpace(in.Name)
	if len(name) > 64 {
		return nil, nil, fmt.Errorf("name must be at most 64 characters")
	}

	entry := &ServiceMap{
		Name:       name,
		Slug:       slug,
		TargetHost: target,
	}
	return entry, in.AllowedGroups, nil
}

func (s *Store) MapSlugConflictsPeer(slug string) (bool, error) {
	var count int64
	err := s.db.Model(&Peer{}).Where("dns_name = ? OR name = ?", slug, slug).Count(&count).Error
	return count > 0, err
}

// MapVirtualIPs returns map VIP strings for netstack local addresses.
func (s *Store) MapVirtualIPs() ([]string, error) {
	var maps []ServiceMap
	if err := s.db.Find(&maps).Error; err != nil {
		return nil, err
	}
	out := make([]string, 0, len(maps))
	for _, r := range maps {
		out = append(out, r.VirtualIP)
	}
	return out, nil
}

// LookupMapVIP resolves a map slug to its virtual IP (integration / legacy helpers).
func (s *Store) LookupMapVIP(slug string) (string, bool) {
	entry, err := s.GetServiceMapBySlug(slug)
	if err != nil {
		return "", false
	}
	return entry.VirtualIP, true
}

func (s *Store) PeerMayAccessMap(peerGroupID, mapID uint) (bool, error) {
	if peerGroupID == 0 {
		return false, nil
	}
	var count int64
	err := s.db.Model(&MapGroupAllow{}).
		Where("relay_id = ? AND group_id = ?", mapID, peerGroupID).
		Count(&count).Error
	return count > 0, err
}

func (s *Store) MapAllowedForPeer(peerWGIP string, mapID uint) (bool, error) {
	peer, err := s.GetPeerByWGIP(peerWGIP)
	if err != nil {
		return false, nil
	}
	return s.PeerMayAccessMap(peer.GroupID, mapID)
}
