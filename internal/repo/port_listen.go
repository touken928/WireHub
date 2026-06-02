package repo

// IsHubListenPortUsed reports whether listenPort is taken by an enabled forward or the hub web/WG port.
func (s *Store) IsHubListenPortUsed(listenPort int) (bool, error) {
	return s.isHubListenPortUsed(listenPort, 0)
}

func (s *Store) isHubListenPortUsed(listenPort int, exceptForwardID uint) (bool, error) {
	var n int64
	q := s.db.Model(&PortForward{}).Where("listen_port = ? AND enabled = ?", listenPort, true)
	if exceptForwardID != 0 {
		q = q.Where("id <> ?", exceptForwardID)
	}
	if err := q.Count(&n).Error; err != nil {
		return false, err
	}
	return n > 0, nil
}
