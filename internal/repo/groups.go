package repo

import (
	"errors"
	"fmt"
	"strings"

	"gorm.io/gorm"
)

func (s *Store) ListGroups() ([]PeerGroup, error) {
	var groups []PeerGroup
	err := s.db.Order("name asc").Find(&groups).Error
	return groups, err
}

func (s *Store) GetGroup(id uint) (*PeerGroup, error) {
	var g PeerGroup
	if err := s.db.First(&g, id).Error; err != nil {
		return nil, err
	}
	return &g, nil
}

func (s *Store) CreateGroup(name string, posX, posY float64) (*PeerGroup, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return nil, fmt.Errorf("group name is required")
	}
	g := &PeerGroup{Name: name, PosX: posX, PosY: posY}
	if err := s.db.Create(g).Error; err != nil {
		return nil, err
	}
	return g, nil
}

func (s *Store) UpdateGroup(g *PeerGroup) error {
	return s.db.Save(g).Error
}

func (s *Store) RenameGroup(id uint, name string) (*PeerGroup, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return nil, fmt.Errorf("group name is required")
	}
	g, err := s.GetGroup(id)
	if err != nil {
		return nil, err
	}
	if g.Name == name {
		return g, nil
	}
	var count int64
	if err := s.db.Model(&PeerGroup{}).Where("name = ? AND id != ?", name, id).Count(&count).Error; err != nil {
		return nil, err
	}
	if count > 0 {
		return nil, fmt.Errorf("group name already exists")
	}
	g.Name = name
	if err := s.db.Save(g).Error; err != nil {
		return nil, err
	}
	return g, nil
}

func (s *Store) DeleteGroup(id uint) error {
	var count int64
	if err := s.db.Model(&Peer{}).Where("group_id = ?", id).Count(&count).Error; err != nil {
		return err
	}
	if count > 0 {
		return fmt.Errorf("group still has peers")
	}
	return s.db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Where("from_group_id = ? OR to_group_id = ?", id, id).Delete(&GroupLink{}).Error; err != nil {
			return err
		}
		if err := tx.Where("group_id = ?", id).Delete(&MapGroupAllow{}).Error; err != nil {
			return err
		}
		return tx.Delete(&PeerGroup{}, id).Error
	})
}

func (s *Store) ListGroupLinks() ([]GroupLink, error) {
	var links []GroupLink
	err := s.db.Find(&links).Error
	return links, err
}

func normalizeLinkPair(fromID, toID uint, bidirectional bool) (uint, uint) {
	if bidirectional && fromID > toID {
		return toID, fromID
	}
	return fromID, toID
}

func (s *Store) deleteLinksBetween(tx *gorm.DB, a, b uint) error {
	return tx.Where(
		"(from_group_id = ? AND to_group_id = ?) OR (from_group_id = ? AND to_group_id = ?)",
		a, b, b, a,
	).Delete(&GroupLink{}).Error
}

func (s *Store) UpsertGroupLink(fromID, toID uint, bidirectional bool) error {
	if fromID == toID {
		return fmt.Errorf("cannot link a group to itself")
	}
	fromID, toID = normalizeLinkPair(fromID, toID, bidirectional)

	return s.db.Transaction(func(tx *gorm.DB) error {
		if err := s.deleteLinksBetween(tx, fromID, toID); err != nil {
			return err
		}
		return tx.Model(&GroupLink{}).Create(map[string]any{
			"from_group_id": fromID,
			"to_group_id":   toID,
			"bidirectional": bidirectional,
		}).Error
	})
}

// FindGroupLink returns the single link between two groups, if any.
func (s *Store) FindGroupLink(a, b uint) (*GroupLink, error) {
	var link GroupLink
	err := s.db.Where(
		"(from_group_id = ? AND to_group_id = ?) OR (from_group_id = ? AND to_group_id = ?)",
		a, b, b, a,
	).First(&link).Error
	if err != nil {
		if errors.Is(err, gorm.ErrRecordNotFound) {
			return nil, nil
		}
		return nil, err
	}
	return &link, nil
}

func (s *Store) HasGroupLink(fromID, toID uint) (bool, error) {
	var count int64
	err := s.db.Model(&GroupLink{}).Where(
		"(from_group_id = ? AND to_group_id = ?) OR (from_group_id = ? AND to_group_id = ?)",
		fromID, toID, toID, fromID,
	).Count(&count).Error
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

func (s *Store) DeleteGroupLink(fromID, toID uint) error {
	return s.db.Where(
		"(from_group_id = ? AND to_group_id = ?) OR (from_group_id = ? AND to_group_id = ?)",
		fromID, toID, toID, fromID,
	).Delete(&GroupLink{}).Error
}

func (s *Store) CountPeersByGroup() (map[uint]int64, error) {
	type row struct {
		GroupID uint
		Count   int64
	}
	var rows []row
	if err := s.db.Model(&Peer{}).Select("group_id, count(*) as count").Group("group_id").Scan(&rows).Error; err != nil {
		return nil, err
	}
	out := make(map[uint]int64, len(rows))
	for _, r := range rows {
		out[r.GroupID] = r.Count
	}
	return out, nil
}

func (s *Store) MigrateGroups() error {
	if err := s.db.AutoMigrate(&PeerGroup{}, &GroupLink{}); err != nil {
		return err
	}
	if s.db.Migrator().HasColumn(&PeerGroup{}, "passive") {
		_ = s.db.Migrator().DropColumn(&PeerGroup{}, "passive")
	}
	if s.db.Migrator().HasTable("port_forward_dmzs") {
		_ = s.db.Migrator().DropTable("port_forward_dmzs")
	}
	if !s.db.Migrator().HasColumn(&Peer{}, "group_id") {
		if err := s.db.Migrator().AddColumn(&Peer{}, "GroupID"); err != nil {
			return err
		}
	}

	var unassigned int64
	if err := s.db.Model(&Peer{}).Where("group_id IS NULL OR group_id = 0").Count(&unassigned).Error; err != nil {
		return err
	}
	if unassigned == 0 {
		return nil
	}

	var defaultGroup PeerGroup
	err := s.db.Where("name = ?", "default").First(&defaultGroup).Error
	if err != nil {
		g, createErr := s.CreateGroup("default", 0, 0)
		if createErr != nil {
			return createErr
		}
		defaultGroup = *g
	}
	return s.db.Model(&Peer{}).Where("group_id IS NULL OR group_id = 0").Update("group_id", defaultGroup.ID).Error
}
