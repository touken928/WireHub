package api

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/touken928/wirehub/internal/store"
)

type groupResponse struct {
	store.PeerGroup
	MemberCount int64 `json:"member_count"`
}

func toGroupResponse(st *store.Store, g store.PeerGroup) (groupResponse, error) {
	count, err := st.CountPeersInGroup(g.ID)
	if err != nil {
		return groupResponse{}, err
	}
	return groupResponse{PeerGroup: g, MemberCount: count}, nil
}

func (s *Server) handleListGroups(c *gin.Context) {
	groups, err := s.store.ListGroups()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	out := make([]groupResponse, 0, len(groups))
	for _, g := range groups {
		resp, err := toGroupResponse(s.store, g)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
		out = append(out, resp)
	}
	c.JSON(http.StatusOK, out)
}

type createGroupRequest struct {
	Name string  `json:"name" binding:"required"`
	PosX float64 `json:"pos_x"`
	PosY float64 `json:"pos_y"`
}

func (s *Server) handleCreateGroup(c *gin.Context) {
	var req createGroupRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	g, err := s.store.CreateGroup(req.Name, req.PosX, req.PosY)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	resp, _ := toGroupResponse(s.store, *g)
	c.JSON(http.StatusCreated, resp)
}

type updateGroupRequest struct {
	Name *string  `json:"name"`
	PosX *float64 `json:"pos_x"`
	PosY *float64 `json:"pos_y"`
}

func (s *Server) handleUpdateGroup(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	g, err := s.store.GetGroup(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "group not found"})
		return
	}
	var req updateGroupRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if req.Name != nil {
		g.Name = *req.Name
	}
	if req.PosX != nil {
		g.PosX = *req.PosX
	}
	if req.PosY != nil {
		g.PosY = *req.PosY
	}
	if err := s.store.UpdateGroup(g); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	resp, _ := toGroupResponse(s.store, *g)
	c.JSON(http.StatusOK, resp)
}

func (s *Server) handleDeleteGroup(c *gin.Context) {
	id, err := parseID(c)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid id"})
		return
	}
	if err := s.store.DeleteGroup(id); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	s.syncAccessFilter()
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

func (s *Server) handleGroupGraph(c *gin.Context) {
	groups, err := s.store.ListGroups()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	links, err := s.store.ListGroupLinks()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	peers, err := s.store.ListPeers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	groupPeers := make(map[uint][]peerResponse, len(groups))
	for _, p := range peers {
		groupPeers[p.GroupID] = append(groupPeers[p.GroupID], toPeerResponse(p))
	}
	groupOut := make([]gin.H, 0, len(groups))
	for _, g := range groups {
		count, _ := s.store.CountPeersInGroup(g.ID)
		groupOut = append(groupOut, gin.H{
			"id":           g.ID,
			"name":         g.Name,
			"pos_x":        g.PosX,
			"pos_y":        g.PosY,
			"member_count": count,
			"peers":        groupPeers[g.ID],
		})
	}
	c.JSON(http.StatusOK, gin.H{
		"groups": groupOut,
		"links":  links,
	})
}

type groupLinkRequest struct {
	FromGroupID uint `json:"from_group_id" binding:"required"`
	ToGroupID   uint `json:"to_group_id" binding:"required"`
}

func (s *Server) handleCreateGroupLink(c *gin.Context) {
	var req groupLinkRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if _, err := s.store.GetGroup(req.FromGroupID); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "from group not found"})
		return
	}
	if _, err := s.store.GetGroup(req.ToGroupID); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "to group not found"})
		return
	}
	if req.FromGroupID == req.ToGroupID {
		c.JSON(http.StatusBadRequest, gin.H{"error": "cannot link a group to itself"})
		return
	}
	exists, err := s.store.HasGroupLink(req.FromGroupID, req.ToGroupID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if exists {
		c.JSON(http.StatusOK, gin.H{"ok": true})
		return
	}
	if err := s.store.UpsertGroupLink(req.FromGroupID, req.ToGroupID); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	s.syncAccessFilter()
	c.JSON(http.StatusCreated, gin.H{"ok": true})
}

func (s *Server) handleDeleteGroupLink(c *gin.Context) {
	var req groupLinkRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := s.store.DeleteGroupLink(req.FromGroupID, req.ToGroupID); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	s.syncAccessFilter()
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

type layoutItem struct {
	ID   uint    `json:"id" binding:"required"`
	PosX float64 `json:"pos_x"`
	PosY float64 `json:"pos_y"`
}

type layoutRequest struct {
	Groups []layoutItem `json:"groups" binding:"required"`
}

func (s *Server) handleUpdateGroupLayout(c *gin.Context) {
	var req layoutRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	for _, item := range req.Groups {
		g, err := s.store.GetGroup(item.ID)
		if err != nil {
			continue
		}
		g.PosX = item.PosX
		g.PosY = item.PosY
		_ = s.store.UpdateGroup(g)
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}
