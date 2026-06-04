package service

import (
	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/domain/policy"
	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/repo"
)

func (a *App) loadSyncBundle() (runtime.SyncBundle, error) {
	settings, err := a.Store.GetSettings()
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	peers, err := a.Store.ListPeers()
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	links, err := a.Store.ListGroupLinks()
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	groups, err := a.Store.ListGroups()
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	mapPolicy, err := a.buildMapAccessPolicy()
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	accessSpec, err := policy.BuildAccessPolicySpec(
		peerEndpoints(peers),
		groupLinkPairs(links),
		policy.NewGroupAccessPolicy(groupAccessList(groups)),
		mapPolicy,
	)
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	forwards, err := a.Store.ListPortForwards()
	if err != nil {
		return runtime.SyncBundle{}, err
	}
	mapDetails, err := a.Store.ListMapDetails()
	if err != nil {
		return runtime.SyncBundle{}, err
	}

	mtu := settings.MTU
	if mtu == 0 {
		mtu = config.DefaultMTU
	}
	statusInterval := settings.StatusInterval
	if statusInterval == 0 {
		statusInterval = config.DefaultStatusInterval
	}

	wgPeers := make([]runtime.WGPeer, len(peers))
	for i, p := range peers {
		wgPeers[i] = runtime.WGPeer{
			ID:        p.ID,
			PublicKey: p.PublicKey,
			WGIP:      p.WGIP,
			DNSName:   p.DNSName,
			GroupID:   p.GroupID,
			Enabled:   p.Enabled,
		}
	}
	fwdRules := make([]runtime.ForwardRule, len(forwards))
	for i, r := range forwards {
		fwdRules[i] = runtime.ForwardRule{
			ID:         r.ID,
			ListenPort: r.ListenPort,
			Protocol:   r.Protocol,
			TargetHost: r.TargetHost,
			TargetPort: r.TargetPort,
		}
	}
	mapRules := make([]runtime.MapRule, len(mapDetails))
	for i, d := range mapDetails {
		mapRules[i] = runtime.MapRule{
			ID:              d.ID,
			Slug:            d.Slug,
			TargetHost:      d.TargetHost,
			VirtualIP:       d.VirtualIP,
			AllowedGroupIDs: policy.AllowedGroupIDSet(d.AllowedGroups),
		}
	}

	netSettings := runtime.NetworkSettings{
		HubIP:            settings.HubIP,
		DNSIP:            settings.DNSIP,
		WGSubnet:         settings.WGSubnet,
		ServerPrivateKey: settings.ServerPrivateKey,
		MTU:              mtu,
		ListenPort:       settings.ListenPort,
		StatusInterval:   statusInterval,
		UpstreamDNS:      settings.UpstreamDNSResolvers(),
	}

	return runtime.SyncBundle{
		Settings: netSettings,
		Peers:    wgPeers,
		Policy:   accessSpec,
		Forwards: fwdRules,
		Maps:     mapRules,
		DNS:      runtime.BuildDNSCatalog(settings.HubIP, wgPeers, mapRules),
	}, nil
}

// ensurePeerDNSRecord creates or refreshes authoritative DNS in the database.
func (a *App) ensurePeerDNSRecord(peer *repo.Peer) error {
	slug := peer.DNSName
	if slug == "" {
		slug = peer.Name
	}
	peer.DNSName = slug
	_ = a.Store.DeleteDNSByPeerID(peer.ID)
	record := &repo.DNSRecord{
		Hostname: slug,
		IP:       peer.WGIP,
		PeerID:   &peer.ID,
		Manual:   false,
	}
	return a.Store.CreateDNSRecord(record)
}

func (a *App) syncDNSCatalog() error {
	dp := a.Hub.dataplane()
	if dp == nil {
		return nil
	}
	bundle, err := a.loadSyncBundle()
	if err != nil {
		return err
	}
	return dp.UpdateDNS(bundle.DNS, bundle.Peers)
}
