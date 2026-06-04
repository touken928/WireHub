package policy

// AllowedGroupIDSet builds a lookup set from map allow-list group IDs.
func AllowedGroupIDSet(ids []uint) map[uint]struct{} {
	if len(ids) == 0 {
		return nil
	}
	out := make(map[uint]struct{}, len(ids))
	for _, id := range ids {
		if id == 0 {
			continue
		}
		out[id] = struct{}{}
	}
	return out
}

// NewMapAccess builds map ACL metadata for a virtual IP and allowed groups.
func NewMapAccess(virtualIP string, allowedGroupIDs []uint) MapAccess {
	return MapAccess{
		VirtualIP:       virtualIP,
		AllowedGroupIDs: AllowedGroupIDSet(allowedGroupIDs),
	}
}

// GroupInAllowedSet reports whether a peer group is in the map allow list.
func GroupInAllowedSet(allowed map[uint]struct{}, groupID uint) bool {
	if groupID == 0 || len(allowed) == 0 {
		return false
	}
	_, ok := allowed[groupID]
	return ok
}
