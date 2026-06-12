package service

import (
	"errors"

	"github.com/touken928/wirehub/internal/repo"
)

var ErrAllowedGroupNotFound = errors.New("allowed group not found")

// ListMapDetails returns all service maps with allowed groups.
func (a *App) ListMapDetails() ([]repo.MapDetail, error) {
	return a.Store.ListMapDetails()
}

// CreateServiceMap adds a map after validating allowed groups exist.
func (a *App) CreateServiceMap(in repo.MapInput) (*repo.MapDetail, error) {
	for _, gid := range in.AllowedGroups {
		if _, err := a.Store.GetGroup(gid); err != nil {
			return nil, ErrAllowedGroupNotFound
		}
	}
	detail, err := a.Store.CreateServiceMap(in)
	if err != nil {
		return nil, err
	}
	if err := a.Hub.SyncMaps(); err != nil {
		return nil, err
	}
	return detail, nil
}

// UpdateServiceMap updates a map and syncs runtime state.
func (a *App) UpdateServiceMap(id uint, in repo.MapInput) (*repo.MapDetail, error) {
	for _, gid := range in.AllowedGroups {
		if _, err := a.Store.GetGroup(gid); err != nil {
			return nil, ErrAllowedGroupNotFound
		}
	}
	detail, err := a.Store.UpdateServiceMap(id, in)
	if err != nil {
		return nil, err
	}
	if err := a.Hub.SyncMaps(); err != nil {
		return nil, err
	}
	return detail, nil
}

// DeleteServiceMap removes a map and syncs runtime state.
func (a *App) DeleteServiceMap(id uint) error {
	if err := a.Store.DeleteServiceMap(id); err != nil {
		return err
	}
	return a.Hub.SyncMaps()
}

// ClassifyMapErr reports whether an error is a slug conflict.
func ClassifyMapErr(err error) bool {
	return errors.Is(err, repo.ErrMapSlugConflict)
}
