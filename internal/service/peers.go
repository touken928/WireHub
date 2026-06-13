package service

import (
	"errors"

	"github.com/touken928/wirehub/internal/domain/client"
	domainpeer "github.com/touken928/wirehub/internal/domain/peer"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// CreatePeer provisions a peer in the database and on the live network stack when running.
func (a *App) CreatePeer(name string, groupID uint) (*repo.Peer, error) {
	slug, err := domainpeer.ValidateHostname(name)
	if err != nil {
		return nil, err
	}

	if _, err := a.store.GetGroup(groupID); err != nil {
		return nil, ErrGroupNotFound
	}

	existing, _ := a.store.ListPeers()
	for _, p := range existing {
		if p.Name == slug {
			return nil, ErrHostnameExists
		}
	}

	settings, err := a.store.GetSettings()
	if err != nil {
		return nil, err
	}

	priv, pub, err := tunnel.GenerateKeyPair()
	if err != nil {
		return nil, err
	}

	ip, err := a.store.AllocateIP(settings.WGSubnet, settings.HubIP, settings.DNSIP)
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

	if err := a.store.CreatePeer(peer); err != nil {
		return nil, err
	}

	if err := a.ensurePeerDNSRecord(peer); err != nil {
		return nil, err
	}
	_ = a.store.UpdatePeer(peer)

	dp := a.Hub.dataplane()
	if dp != nil {
		if err := dp.SyncPeer(repoPeerToWG(peer)); err != nil {
			return nil, err
		}
		if err := a.syncDNSCatalog(); err != nil {
			return nil, err
		}
	}
	if err := a.SyncAccessFilter(); err != nil {
		return nil, err
	}
	return peer, nil
}

// sentinel errors for peer operations
var (
	ErrPeerNotFound   = errors.New("peer not found")
	ErrGroupNotFound  = errors.New("group not found")
	ErrHostnameExists = errors.New("hostname already exists")
)

// UpdatePeerFields atomically renames and/or moves a peer. Nil fields are left unchanged.
func (a *App) UpdatePeerFields(id uint, name *string, groupID *uint) (*repo.Peer, error) {
	peer, err := a.store.GetPeer(id)
	if err != nil {
		return nil, ErrPeerNotFound
	}
	if groupID != nil {
		if _, err := a.store.GetGroup(*groupID); err != nil {
			return nil, ErrGroupNotFound
		}
	}
	changed := false
	if name != nil {
		slug, err := domainpeer.ValidateHostname(*name)
		if err != nil {
			return nil, err
		}
		if peer.Name != slug {
			existing, _ := a.store.ListPeers()
			for _, p := range existing {
				if p.ID != id && p.Name == slug {
					return nil, ErrHostnameExists
				}
			}
			peer.Name = slug
			peer.DNSName = slug
			changed = true
		}
	}
	if groupID != nil && peer.GroupID != *groupID {
		peer.GroupID = *groupID
		changed = true
	}
	if !changed {
		return peer, nil
	}
	if err := a.store.UpdatePeer(peer); err != nil {
		return nil, err
	}
	if name != nil {
		if err := a.ensurePeerDNSRecord(peer); err != nil {
			return nil, err
		}
	}
	dp := a.Hub.dataplane()
	if dp != nil {
		if err := dp.SyncPeer(repoPeerToWG(peer)); err != nil {
			return nil, err
		}
		if name != nil {
			if err := a.syncDNSCatalog(); err != nil {
				return nil, err
			}
		}
	}
	if err := a.SyncAccessFilter(); err != nil {
		return nil, err
	}
	return peer, nil
}

// DeletePeer removes a peer from the database and live network.
func (a *App) DeletePeer(peerID uint) error {
	peer, err := a.store.GetPeer(peerID)
	if err != nil {
		return ErrPeerNotFound
	}
	if dp := a.Hub.dataplane(); dp != nil {
		_ = dp.RemovePeer(peer.PublicKey)
	}
	_ = a.store.DeleteDNSByPeerID(peerID)
	if err := a.store.DeletePeer(peerID); err != nil {
		return err
	}
	if err := a.syncDNSCatalog(); err != nil {
		return err
	}
	return a.SyncAccessFilter()
}

// TogglePeer enables or disables a peer on the live network.
func (a *App) TogglePeer(peerID uint) (*repo.Peer, error) {
	peer, err := a.store.GetPeer(peerID)
	if err != nil {
		return nil, ErrPeerNotFound
	}
	peer.Enabled = !peer.Enabled
	if err := a.store.UpdatePeer(peer); err != nil {
		return nil, err
	}
	dp := a.Hub.dataplane()
	if dp != nil {
		if peer.Enabled {
			if err := dp.SyncPeer(repoPeerToWG(peer)); err != nil {
				return nil, err
			}
		} else {
			_ = dp.RemovePeer(peer.PublicKey)
		}
		if err := a.syncDNSCatalog(); err != nil {
			return nil, err
		}
	}
	if err := a.SyncAccessFilter(); err != nil {
		return nil, err
	}
	return peer, nil
}

// ClientConfig renders the WireGuard client config for a peer.
func (a *App) ClientConfig(peerID uint) (string, error) {
	peer, err := a.store.GetPeer(peerID)
	if err != nil {
		return "", ErrPeerNotFound
	}
	settings, err := a.store.GetSettings()
	if err != nil {
		return "", err
	}
	return client.BuildClientConfig(client.ClientConfigInput{
		Endpoint:        settings.Endpoint,
		ListenPort:      settings.ListenPort,
		ServerPublicKey: settings.ServerPublicKey,
		AllowedSubnet:   settings.WGSubnet,
		ClientDNS:       settings.ClientDNS(),
		PeerPrivateKey:  peer.PrivateKey,
		PeerAddress:     peer.WGIP,
	})
}
