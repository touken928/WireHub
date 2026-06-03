import type { PeerStatus } from '@/api/types';

export type PeerConnectionFilter = 'all' | 'online' | 'offline' | 'disabled';

export type PeerListFilters = {
  query: string;
  groupId: number | null;
  status: PeerConnectionFilter;
};

function peerSearchText(peer: PeerStatus): string {
  return [peer.name, peer.fqdn, peer.wg_ip, peer.group_name].join(' ').toLowerCase();
}

function matchesStatus(peer: PeerStatus, status: PeerConnectionFilter): boolean {
  switch (status) {
    case 'online':
      return peer.enabled && peer.online;
    case 'offline':
      return peer.enabled && !peer.online;
    case 'disabled':
      return !peer.enabled;
    default:
      return true;
  }
}

export function filterPeers(peers: PeerStatus[], filters: PeerListFilters): PeerStatus[] {
  const q = filters.query.trim().toLowerCase();
  return peers.filter((peer) => {
    if (filters.groupId != null && peer.group_id !== filters.groupId) {
      return false;
    }
    if (!matchesStatus(peer, filters.status)) {
      return false;
    }
    if (q && !peerSearchText(peer).includes(q)) {
      return false;
    }
    return true;
  });
}

export function hasActivePeerFilters(filters: PeerListFilters): boolean {
  return filters.query.trim() !== '' || filters.groupId != null || filters.status !== 'all';
}
