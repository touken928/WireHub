package service

import (
	"fmt"

	"github.com/touken928/wirehub/internal/domain"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/wg"
)

// CreatePeer provisions a peer in the database and on the live network stack when running.
func (h *Hub) CreatePeer(name string, groupID uint) (*repo.Peer, error) {
	slug, err := domain.ValidateHostname(name)
	if err != nil {
		return nil, err
	}

	if _, err := h.Store.GetGroup(groupID); err != nil {
		return nil, fmt.Errorf("group not found")
	}

	existing, _ := h.Store.ListPeers()
	for _, p := range existing {
		if p.Name == slug {
			return nil, fmt.Errorf("hostname already exists")
		}
	}

	settings, err := h.Store.GetSettings()
	if err != nil {
		return nil, err
	}

	priv, pub, err := wg.GenerateKeyPair()
	if err != nil {
		return nil, err
	}

	ip, err := h.Store.AllocateIP(settings.WGSubnet, settings.HubIP)
	if err != nil {
		return nil, err
	}

	peer := &repo.Peer{
		Name:       slug,
		PublicKey:  pub,
		PrivateKey: priv,
		WGIP:       ip,
		GroupID:    groupID,
		Enabled:    true,
		DNSName:    slug,
	}

	if err := h.Store.CreatePeer(peer); err != nil {
		return nil, err
	}

	wgMgr, err := h.wgManager()
	if err != nil {
		return nil, err
	}
	if err := wgMgr.SyncPeer(peer); err != nil {
		return nil, err
	}
	if dns, err := h.dnsServer(); err == nil {
		_ = dns.RegisterPeer(peer)
	}
	_ = h.Store.UpdatePeer(peer)
	h.SyncAccessFilter()
	return peer, nil
}

// UpdatePeerGroup moves a peer to another group and refreshes the network stack.
func (h *Hub) UpdatePeerGroup(peerID, groupID uint) (*repo.Peer, error) {
	peer, err := h.Store.GetPeer(peerID)
	if err != nil {
		return nil, fmt.Errorf("peer not found")
	}
	if _, err := h.Store.GetGroup(groupID); err != nil {
		return nil, fmt.Errorf("group not found")
	}
	peer.GroupID = groupID

	if err := h.Store.UpdatePeer(peer); err != nil {
		return nil, err
	}
	wgMgr, err := h.wgManager()
	if err != nil {
		return nil, err
	}
	if err := wgMgr.SyncPeer(peer); err != nil {
		return nil, err
	}
	h.SyncAccessFilter()
	return peer, nil
}

// DeletePeer removes a peer from the database and live network.
func (h *Hub) DeletePeer(peerID uint) error {
	peer, err := h.Store.GetPeer(peerID)
	if err != nil {
		return fmt.Errorf("peer not found")
	}
	if wgMgr, err := h.wgManager(); err == nil {
		_ = wgMgr.RemovePeer(peer.PublicKey)
	}
	_ = h.Store.DeleteDNSByPeerID(peerID)
	if err := h.Store.DeletePeer(peerID); err != nil {
		return err
	}
	h.SyncAccessFilter()
	return nil
}

// TogglePeer enables or disables a peer on the live network.
func (h *Hub) TogglePeer(peerID uint) (*repo.Peer, error) {
	peer, err := h.Store.GetPeer(peerID)
	if err != nil {
		return nil, fmt.Errorf("peer not found")
	}
	peer.Enabled = !peer.Enabled
	if err := h.Store.UpdatePeer(peer); err != nil {
		return nil, err
	}
	wgMgr, err := h.wgManager()
	if err != nil {
		return nil, err
	}
	if peer.Enabled {
		if err := wgMgr.SyncPeer(peer); err != nil {
			return nil, err
		}
	} else {
		_ = wgMgr.RemovePeer(peer.PublicKey)
	}
	h.SyncAccessFilter()
	return peer, nil
}

// ClientConfig renders the WireGuard client config for a peer.
func (h *Hub) ClientConfig(peerID uint) (string, error) {
	peer, err := h.Store.GetPeer(peerID)
	if err != nil {
		return "", fmt.Errorf("peer not found")
	}
	settings, err := h.Store.GetSettings()
	if err != nil {
		return "", err
	}
	return domain.BuildClientConfig(domain.ClientConfigInput{
		Endpoint:        settings.Endpoint,
		ListenPort:      settings.ListenPort,
		ServerPublicKey: settings.ServerPublicKey,
		AllowedSubnet:   settings.WGSubnet,
		ClientDNS:       settings.ClientDNS(),
		PeerPrivateKey:  peer.PrivateKey,
		PeerAddress:     peer.WGIP,
	})
}
