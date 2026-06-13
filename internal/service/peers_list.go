package service

import "github.com/touken928/wirehub/internal/repo"

// ListPeers returns all peers from persistence.
func (a *App) ListPeers() ([]repo.Peer, error) {
	return a.store.ListPeers()
}

// GetPeer loads one peer by id.
func (a *App) GetPeer(id uint) (*repo.Peer, error) {
	return a.store.GetPeer(id)
}
