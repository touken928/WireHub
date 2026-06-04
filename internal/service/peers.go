package service

import (
	"fmt"

	"github.com/touken928/wirehub/internal/domain/client"
	"github.com/touken928/wirehub/internal/domain/peer"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
)

// CreatePeer provisions a peer in the database and on the live network stack when running.
func (a *App) CreatePeer(name string, groupID uint) (*repo.Peer, error) {
	slug, err := peer.ValidateHostname(name)
	if err != nil {
		return nil, err
	}

	if _, err := a.Store.GetGroup(groupID); err != nil {
		return nil, fmt.Errorf("group not found")
	}

	existing, _ := a.Store.ListPeers()
	for _, p := range existing {
		if p.Name == slug {
			return nil, fmt.Errorf("hostname already exists")
		}
	}

	settings, err := a.Store.GetSettings()
	if err != nil {
		return nil, err
	}

	priv, pub, err := tunnel.GenerateKeyPair()
	if err != nil {
		return nil, err
	}

	ip, err := a.Store.AllocateIP(settings.WGSubnet, settings.HubIP)
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

	if err := a.Store.CreatePeer(peer); err != nil {
		return nil, err
	}

	if err := a.ensurePeerDNSRecord(peer); err != nil {
		return nil, err
	}
	_ = a.Store.UpdatePeer(peer)

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

// RenamePeer changes a peer hostname and refreshes authoritative DNS.
func (a *App) RenamePeer(peerID uint, name string) (*repo.Peer, error) {
	slug, err := peer.ValidateHostname(name)
	if err != nil {
		return nil, err
	}

	peer, err := a.Store.GetPeer(peerID)
	if err != nil {
		return nil, fmt.Errorf("peer not found")
	}
	if peer.Name == slug {
		return peer, nil
	}

	existing, _ := a.Store.ListPeers()
	for _, p := range existing {
		if p.ID != peerID && p.Name == slug {
			return nil, fmt.Errorf("hostname already exists")
		}
	}

	peer.Name = slug
	peer.DNSName = slug
	if err := a.Store.UpdatePeer(peer); err != nil {
		return nil, err
	}
	if err := a.ensurePeerDNSRecord(peer); err != nil {
		return nil, err
	}
	if err := a.syncDNSCatalog(); err != nil {
		return nil, err
	}
	if err := a.SyncAccessFilter(); err != nil {
		return nil, err
	}
	return peer, nil
}

// UpdatePeerGroup moves a peer to another group and refreshes the network stack.
func (a *App) UpdatePeerGroup(peerID, groupID uint) (*repo.Peer, error) {
	peer, err := a.Store.GetPeer(peerID)
	if err != nil {
		return nil, fmt.Errorf("peer not found")
	}
	if _, err := a.Store.GetGroup(groupID); err != nil {
		return nil, fmt.Errorf("group not found")
	}
	peer.GroupID = groupID

	if err := a.Store.UpdatePeer(peer); err != nil {
		return nil, err
	}
	dp := a.Hub.dataplane()
	if dp != nil {
		if err := dp.SyncPeer(repoPeerToWG(peer)); err != nil {
			return nil, err
		}
	}
	if err := a.SyncAccessFilter(); err != nil {
		return nil, err
	}
	return peer, nil
}

// DeletePeer removes a peer from the database and live network.
func (a *App) DeletePeer(peerID uint) error {
	peer, err := a.Store.GetPeer(peerID)
	if err != nil {
		return fmt.Errorf("peer not found")
	}
	if dp := a.Hub.dataplane(); dp != nil {
		_ = dp.RemovePeer(peer.PublicKey)
	}
	_ = a.Store.DeleteDNSByPeerID(peerID)
	if err := a.Store.DeletePeer(peerID); err != nil {
		return err
	}
	if err := a.syncDNSCatalog(); err != nil {
		return err
	}
	return a.SyncAccessFilter()
}

// TogglePeer enables or disables a peer on the live network.
func (a *App) TogglePeer(peerID uint) (*repo.Peer, error) {
	peer, err := a.Store.GetPeer(peerID)
	if err != nil {
		return nil, fmt.Errorf("peer not found")
	}
	peer.Enabled = !peer.Enabled
	if err := a.Store.UpdatePeer(peer); err != nil {
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
	peer, err := a.Store.GetPeer(peerID)
	if err != nil {
		return "", fmt.Errorf("peer not found")
	}
	settings, err := a.Store.GetSettings()
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
