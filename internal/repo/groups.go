package repo

import (
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
		return tx.Delete(&PeerGroup{}, id).Error
	})
}

func (s *Store) ListGroupLinks() ([]GroupLink, error) {
	var links []GroupLink
	err := s.db.Find(&links).Error
	return links, err
}

func (s *Store) UpsertGroupLink(fromID, toID uint) error {
	if fromID == toID {
		return fmt.Errorf("cannot link a group to itself")
	}
	if fromID > toID {
		fromID, toID = toID, fromID
	}
	var count int64
	if err := s.db.Model(&GroupLink{}).
		Where("from_group_id = ? AND to_group_id = ?", fromID, toID).
		Count(&count).Error; err != nil {
		return err
	}
	if count > 0 {
		return nil
	}
	link := GroupLink{FromGroupID: fromID, ToGroupID: toID}
	return s.db.Create(&link).Error
}

func (s *Store) HasGroupLink(fromID, toID uint) (bool, error) {
	if fromID > toID {
		fromID, toID = toID, fromID
	}
	var count int64
	err := s.db.Model(&GroupLink{}).
		Where("from_group_id = ? AND to_group_id = ?", fromID, toID).
		Count(&count).Error
	return count > 0, err
}

func (s *Store) DeleteGroupLink(fromID, toID uint) error {
	if fromID > toID {
		fromID, toID = toID, fromID
	}
	return s.db.Where("from_group_id = ? AND to_group_id = ?", fromID, toID).Delete(&GroupLink{}).Error
}

func (s *Store) CountPeersInGroup(groupID uint) (int64, error) {
	var count int64
	err := s.db.Model(&Peer{}).Where("group_id = ?", groupID).Count(&count).Error
	return count, err
}

func (s *Store) MigrateGroups() error {
	if err := s.db.AutoMigrate(&PeerGroup{}, &GroupLink{}); err != nil {
		return err
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
