import type { PeerGroup } from '@/api/types';

export const ALLOWED_GROUPS_REQUIRED = 'Select at least one allowed group';

export function toggleAllowedGroup(ids: number[], groupId: number, checked: boolean): number[] {
  if (checked) {
    return ids.includes(groupId) ? ids : [...ids, groupId];
  }
  return ids.filter((id) => id !== groupId);
}

export function selectAllGroupIds(groups: readonly { id: number }[]): number[] {
  return groups.map((g) => g.id);
}

export function clearAllowedGroups(): number[] {
  return [];
}

export function groupsForAllowedIds(ids: number[], groups: readonly PeerGroup[]): PeerGroup[] {
  const byId = new Map(groups.map((g) => [g.id, g]));
  return ids
    .map((id) => byId.get(id))
    .filter((g): g is PeerGroup => g != null)
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function allowedGroupsSummary(selectedCount: number, total: number): string {
  if (total === 0) {
    return 'No groups in hub';
  }
  if (selectedCount === 0) {
    return 'No groups selected — map is unreachable';
  }
  if (selectedCount === total) {
    return `All ${total} groups allowed`;
  }
  return `${selectedCount} of ${total} groups allowed`;
}
