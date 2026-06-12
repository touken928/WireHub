import type { PeerStatus } from '@/api/types';
import type { PeerMemberCardPeer } from '@/components/peers/PeerMemberCard';
import type { EnrichedPeer } from '@/pages/groups/utils';

export function statusToMemberCardPeer(peer: PeerStatus): PeerMemberCardPeer {
  return {
    id: peer.id,
    name: peer.name,
    fqdn: peer.fqdn,
    wg_ip: peer.wg_ip,
    group_id: peer.group_id,
    group_name: peer.group_name,
    enabled: peer.enabled,
    online: peer.online,
    last_handshake: peer.last_handshake,
    rx_bytes: peer.rx_bytes,
    tx_bytes: peer.tx_bytes,
  };
}

export function enrichedPeerToMemberCardPeer(peer: EnrichedPeer): PeerMemberCardPeer {
  return {
    id: peer.id,
    name: peer.name,
    fqdn: peer.fqdn,
    wg_ip: peer.wg_ip,
    group_id: peer.group_id,
    enabled: peer.enabled,
    online: peer.online,
    last_handshake: peer.last_handshake ?? 0,
    rx_bytes: peer.rx_bytes ?? 0,
    tx_bytes: peer.tx_bytes ?? 0,
  };
}
