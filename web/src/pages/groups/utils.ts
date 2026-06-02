import type { GroupGraphNode, GroupGraphPeer, PeerStatus } from '@/api/types';

export type EnrichedPeer = GroupGraphPeer & Partial<PeerStatus>;

export function mergePeerStatus(peers: GroupGraphPeer[], status: PeerStatus[]): EnrichedPeer[] {
  const byId = new Map(status.map((p) => [p.id, p]));
  return peers.map((p) => ({ ...p, ...byId.get(p.id) }));
}

export function pickLargestGroupId(groups: GroupGraphNode[]): number | null {
  if (groups.length === 0) return null;
  let best = groups[0];
  for (const group of groups) {
    const count = group.member_count ?? 0;
    const bestCount = best.member_count ?? 0;
    if (count > bestCount || (count === bestCount && group.id < best.id)) {
      best = group;
    }
  }
  return best.id;
}
