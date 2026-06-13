package service

import (
	"errors"

	"github.com/touken928/wirehub/internal/repo"
)

// GroupView is a peer group with member count.
type GroupView struct {
	repo.PeerGroup
	MemberCount int64
}

// ListGroups returns all groups with member counts.
func (a *App) ListGroups() ([]GroupView, error) {
	groups, err := a.Store.ListGroups()
	if err != nil {
		return nil, err
	}
	counts, err := a.Store.CountPeersByGroup()
	if err != nil {
		return nil, err
	}
	out := make([]GroupView, 0, len(groups))
	for _, g := range groups {
		out = append(out, GroupView{PeerGroup: g, MemberCount: counts[g.ID]})
	}
	return out, nil
}

// GetGroupNameMap returns all group IDs to their display name.
func (a *App) GetGroupNameMap() map[uint]string {
	groups, err := a.Store.ListGroups()
	if err != nil {
		return nil
	}
	out := make(map[uint]string, len(groups))
	for _, g := range groups {
		out[g.ID] = g.Name
	}
	return out
}

// CreateGroup adds a new peer group.
func (a *App) CreateGroup(name string, posX, posY float64) (*repo.PeerGroup, error) {
	return a.Store.CreateGroup(name, posX, posY)
}

// GetGroup loads a group by id.
func (a *App) GetGroup(id uint) (*repo.PeerGroup, error) {
	return a.Store.GetGroup(id)
}

// UpdateGroup persists group fields.
func (a *App) UpdateGroup(g *repo.PeerGroup) error {
	return a.Store.UpdateGroup(g)
}

// RenameGroup changes a group's display name.
func (a *App) RenameGroup(id uint, name string) (*repo.PeerGroup, error) {
	return a.Store.RenameGroup(id, name)
}

// DeleteGroup removes a group and refreshes ACL rules.
func (a *App) DeleteGroup(id uint) error {
	if err := a.Store.DeleteGroup(id); err != nil {
		return err
	}
	return a.SyncAccessFilter()
}

// GroupGraphData holds groups, links, and peers for the topology UI.
type GroupGraphData struct {
	Groups []repo.PeerGroup
	Links  []repo.GroupLink
	Peers  []repo.Peer
}

// GroupGraph returns data for the groups canvas.
func (a *App) GroupGraph() (GroupGraphData, error) {
	groups, err := a.Store.ListGroups()
	if err != nil {
		return GroupGraphData{}, err
	}
	links, err := a.Store.ListGroupLinks()
	if err != nil {
		return GroupGraphData{}, err
	}
	peers, err := a.Store.ListPeers()
	if err != nil {
		return GroupGraphData{}, err
	}
	return GroupGraphData{Groups: groups, Links: links, Peers: peers}, nil
}

// CreateGroupLink adds or replaces a directed group link.
func (a *App) CreateGroupLink(fromID, toID uint, bidirectional bool) error {
	if _, err := a.Store.GetGroup(fromID); err != nil {
		return err
	}
	if _, err := a.Store.GetGroup(toID); err != nil {
		return err
	}
	if fromID == toID {
		return ErrSelfLink
	}
	if err := a.Store.UpsertGroupLink(fromID, toID, bidirectional); err != nil {
		return err
	}
	return a.SyncAccessFilter()
}

// DeleteGroupLink removes a directed group link.
func (a *App) DeleteGroupLink(fromID, toID uint) error {
	if err := a.Store.DeleteGroupLink(fromID, toID); err != nil {
		return err
	}
	return a.SyncAccessFilter()
}

// UpdateGroupLayout saves canvas positions for groups.
func (a *App) UpdateGroupLayout(items []GroupLayoutItem) error {
	for _, item := range items {
		g, err := a.Store.GetGroup(item.ID)
		if err != nil {
			continue
		}
		g.PosX = item.PosX
		g.PosY = item.PosY
		_ = a.Store.UpdateGroup(g)
	}
	return nil
}

// GroupLayoutItem is one node position on the groups graph.
type GroupLayoutItem struct {
	ID   uint
	PosX float64
	PosY float64
}

// UpdateGroupFields applies name, position, and intra-group policy changes.
func (a *App) UpdateGroupFields(id uint, name *string, posX, posY *float64, allowIntra *bool) (*repo.PeerGroup, bool, error) {
	g, err := a.Store.GetGroup(id)
	if err != nil {
		return nil, false, err
	}
	if name != nil {
		g, err = a.Store.RenameGroup(id, *name)
		if err != nil {
			return nil, false, err
		}
	}
	if posX != nil {
		g.PosX = *posX
	}
	if posY != nil {
		g.PosY = *posY
	}
	if allowIntra != nil {
		g.AllowIntraGroup = *allowIntra
	}
	needsSave := posX != nil || posY != nil || allowIntra != nil
	if needsSave {
		if err := a.Store.UpdateGroup(g); err != nil {
			return nil, false, err
		}
		if err := a.SyncAccessFilter(); err != nil {
			return g, true, err
		}
	}
	return g, needsSave, nil
}

// ErrSelfLink is returned when linking a group to itself.
var ErrSelfLink = errors.New("cannot link a group to itself")
