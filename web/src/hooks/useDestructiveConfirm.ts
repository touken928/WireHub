import { useCallback } from 'react';
import { useConfirm } from '@/components/common/useConfirm';

export function useDestructiveConfirm() {
  const { confirm } = useConfirm();

  const confirmDeleteGroup = useCallback(
    () => confirm({
      title: 'Delete group?',
      message: 'Peers must be removed before this group can be deleted.',
      confirmLabel: 'Delete',
    }),
    [confirm],
  );

  const confirmDeletePeer = useCallback(
    (name?: string) => {
      const label = name?.trim() ? `"${name.trim()}"` : 'this peer';
      return confirm({
        title: 'Delete peer?',
        message: `Delete ${label}? This cannot be undone.`,
        confirmLabel: 'Delete',
      });
    },
    [confirm],
  );

  const confirmDisconnectLinks = useCallback(
    (count = 1) => confirm({
      title: count <= 1 ? 'Disconnect link?' : `Disconnect ${count} links?`,
      message: 'Peers will lose cross-group access between these groups.',
      confirmLabel: 'Disconnect',
    }),
    [confirm],
  );

  return {
    confirmDeleteGroup,
    confirmDeletePeer,
    confirmDisconnectLinks,
  };
}
