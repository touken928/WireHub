import {
  Button,
  Field,
  Input,
  Subtitle1,
  Switch,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { EditRegular } from '@fluentui/react-icons';
import { useState } from 'react';
import type { GroupGraphNode } from '@/api/types';
import { RenameDialog } from '@/components/common/RenameDialog';
import { PeerMemberCard, type PeerMemberCardGroup } from '@/components/peers/PeerMemberCard';
import { getErrorMessage } from '@/lib/error';
import { enrichedPeerToMemberCardPeer } from '@/lib/peerAdapter';
import type { EnrichedPeer } from '@/pages/groups/utils';

const useStyles = makeStyles({
  panel: {
    flex: '0 0 420px',
    display: 'grid',
    gridTemplateRows: 'auto minmax(0, 1fr) auto',
    minHeight: 0,
    alignSelf: 'stretch',
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow4,
    overflow: 'hidden',
  },
  panelEmpty: {
    display: 'flex',
    gridTemplateRows: 'unset',
    alignItems: 'center',
    justifyContent: 'center',
  },
  header: {
    flexShrink: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
    padding: '14px 16px',
    borderBottom: `1px solid ${tokens.colorNeutralStroke2}`,
  },
  headerRow: {
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
  },
  groupName: {
    flex: 1,
    minWidth: 0,
    margin: 0,
  },
  intraRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '12px',
  },
  peerList: {
    minHeight: 0,
    overflowY: 'auto',
    overflowX: 'hidden',
    overscrollBehavior: 'contain',
    WebkitOverflowScrolling: 'touch',
    padding: '12px 16px',
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  addSection: {
    flexShrink: 0,
    padding: '12px 16px 16px',
    display: 'flex',
    flexDirection: 'column',
    gap: '8px',
    borderTop: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
  },
  panelEmptyText: {
    padding: '24px 16px',
    textAlign: 'center',
  },
  addRow: {
    display: 'flex',
    gap: '8px',
  },
  empty: {
    padding: '24px 8px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
  },
});

type GroupDetailPanelProps = {
  group: GroupGraphNode | null;
  groups: PeerMemberCardGroup[];
  peers: EnrichedPeer[];
  mutedClassName: string;
  newPeerName: string;
  createPeerError: string;
  onNewPeerNameChange: (value: string) => void;
  onCreatePeer: () => void;
  onRenameGroup: (name: string) => Promise<void>;
  onAllowIntraGroupChange: (allow: boolean) => Promise<void>;
  onRenamePeer: (peerId: number, name: string) => Promise<void>;
  onMovePeer: (peerId: number, groupId: number) => Promise<void>;
  onShowConfig: (peerId: number) => void;
  onTogglePeer: (peerId: number) => void;
  onDeletePeer: (peerId: number, peerName: string) => void;
};

export function GroupDetailPanel({
  group,
  groups,
  peers,
  mutedClassName,
  newPeerName,
  createPeerError,
  onNewPeerNameChange,
  onCreatePeer,
  onRenameGroup,
  onAllowIntraGroupChange,
  onRenamePeer,
  onMovePeer,
  onShowConfig,
  onTogglePeer,
  onDeletePeer,
}: GroupDetailPanelProps) {
  const styles = useStyles();
  const [groupRenameOpen, setGroupRenameOpen] = useState(false);
  const [groupRenameValue, setGroupRenameValue] = useState('');
  const [groupRenameError, setGroupRenameError] = useState('');

  const openGroupRename = () => {
    if (!group) return;
    setGroupRenameValue(group.name);
    setGroupRenameError('');
    setGroupRenameOpen(true);
  };

  const saveGroupRename = async () => {
    const name = groupRenameValue.trim();
    if (!name) return;
    setGroupRenameError('');
    try {
      await onRenameGroup(name);
      setGroupRenameOpen(false);
    } catch (err) {
      setGroupRenameError(getErrorMessage(err, 'Rename failed'));
    }
  };

  if (!group) {
    return (
      <aside className={`${styles.panel} ${styles.panelEmpty}`}>
        <div className={styles.panelEmptyText}>
          <Text size={300}>No groups yet. Add a group to get started.</Text>
        </div>
      </aside>
    );
  }

  return (
    <aside className={styles.panel}>
      <div className={styles.header}>
        <div className={styles.headerRow}>
          <Subtitle1 block truncate className={styles.groupName}>
            {group.name}
          </Subtitle1>
          <Button
            size="small"
            appearance="subtle"
            icon={<EditRegular />}
            aria-label="Rename group"
            onClick={openGroupRename}
          />
          <Text size={200} className={mutedClassName}>{peers.length} peer(s)</Text>
        </div>
        <div className={styles.intraRow}>
          <Text size={300}>Same-group interconnect</Text>
          <Switch
            checked={group.allow_intra_group !== false}
            aria-label="Allow peers in this group to reach each other"
            onChange={(_, data) => void onAllowIntraGroupChange(Boolean(data.checked))}
          />
        </div>
      </div>
      <div className={styles.peerList}>
        {peers.length === 0 ? (
          <div className={styles.empty}>
            <Text size={300}>No peers in this group yet.</Text>
          </div>
        ) : (
          peers.map((peer) => (
            <PeerMemberCard
              key={peer.id}
              peer={enrichedPeerToMemberCardPeer(peer)}
              groups={groups}
              onRename={onRenamePeer}
              onMove={onMovePeer}
              onShowConfig={onShowConfig}
              onToggle={onTogglePeer}
              onDelete={onDeletePeer}
            />
          ))
        )}
      </div>
      <div className={styles.addSection}>
        <Text weight="semibold" size={300}>Add peer</Text>
        <div className={styles.addRow}>
          <Field style={{ flex: 1 }}>
            <Input
              value={newPeerName}
              placeholder="alice"
              onChange={(_, data) => onNewPeerNameChange(data.value)}
            />
          </Field>
          <Button appearance="primary" onClick={onCreatePeer} disabled={!newPeerName.trim()}>
            Add
          </Button>
        </div>
        {createPeerError && (
          <Text size={200} style={{ color: tokens.colorPaletteRedForeground1 }}>{createPeerError}</Text>
        )}
      </div>

      <RenameDialog
        open={groupRenameOpen}
        title="Rename group"
        label="Group name"
        value={groupRenameValue}
        error={groupRenameError}
        onValueChange={setGroupRenameValue}
        onClose={() => setGroupRenameOpen(false)}
        onSave={() => void saveGroupRename()}
      />
    </aside>
  );
}
