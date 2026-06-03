import {
  Button,
  Card,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  Select,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import {
  ArrowDownloadRegular,
  DeleteRegular,
  EditRegular,
  PeopleTeamRegular,
  PowerRegular,
} from '@fluentui/react-icons';
import { useState } from 'react';
import type { GroupGraphNode } from '@/api/types';
import { DNS_DOMAIN } from '@/constants';
import { RenameDialog } from '@/components/common/RenameDialog';
import { formatBytes, formatHandshake } from '@/lib/format';
import { getErrorMessage } from '@/lib/error';
import { PeerStatusBadge } from '@/components/peers/PeerStatusBadge';
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
    alignItems: 'center',
    gap: '10px',
    padding: '14px 16px',
    borderBottom: `1px solid ${tokens.colorNeutralStroke2}`,
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
  peerCard: {
    flexShrink: 0,
    padding: '14px',
    display: 'flex',
    flexDirection: 'column',
    gap: '10px',
    borderRadius: tokens.borderRadiusLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground2,
  },
  peerHeader: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: '8px',
  },
  peerTop: {
    flex: 1,
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    flexWrap: 'wrap',
  },
  peerMeta: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: '8px 12px',
  },
  metaItem: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
  },
  metaLabel: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase200,
  },
  peerActions: {
    display: 'grid',
    gridTemplateColumns: 'repeat(4, minmax(0, 1fr))',
    gap: '6px',
  },
  actionButton: {
    width: '100%',
    minWidth: 0,
    paddingLeft: '6px',
    paddingRight: '6px',
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

type GroupOption = {
  id: number;
  name: string;
};

type GroupDetailPanelProps = {
  group: GroupGraphNode | null;
  groups: GroupOption[];
  peers: EnrichedPeer[];
  mutedClassName: string;
  newPeerName: string;
  createPeerError: string;
  onNewPeerNameChange: (value: string) => void;
  onCreatePeer: () => void;
  onRenameGroup: (name: string) => Promise<void>;
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
  const [peerRenameOpen, setPeerRenameOpen] = useState(false);
  const [peerRenameId, setPeerRenameId] = useState<number | null>(null);
  const [peerRenameValue, setPeerRenameValue] = useState('');
  const [peerRenameError, setPeerRenameError] = useState('');
  const [moveOpen, setMoveOpen] = useState(false);
  const [movePeerId, setMovePeerId] = useState<number | null>(null);
  const [movePeerName, setMovePeerName] = useState('');
  const [moveGroupId, setMoveGroupId] = useState('');
  const [moveError, setMoveError] = useState('');

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

  const openPeerRename = (peerId: number, currentName: string) => {
    setPeerRenameId(peerId);
    setPeerRenameValue(currentName);
    setPeerRenameError('');
    setPeerRenameOpen(true);
  };

  const savePeerRename = async () => {
    const name = peerRenameValue.trim();
    if (!name || peerRenameId == null) return;
    setPeerRenameError('');
    try {
      await onRenamePeer(peerRenameId, name);
      setPeerRenameOpen(false);
      setPeerRenameId(null);
    } catch (err) {
      setPeerRenameError(getErrorMessage(err, 'Rename failed'));
    }
  };

  const openMovePeer = (peerId: number, peerName: string, currentGroupId: number) => {
    setMovePeerId(peerId);
    setMovePeerName(peerName);
    setMoveGroupId(String(currentGroupId));
    setMoveError('');
    setMoveOpen(true);
  };

  const saveMovePeer = async () => {
    if (movePeerId == null || !moveGroupId) return;
    setMoveError('');
    try {
      await onMovePeer(movePeerId, Number(moveGroupId));
      setMoveOpen(false);
      setMovePeerId(null);
    } catch (err) {
      setMoveError(getErrorMessage(err, 'Move failed'));
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
        <Text weight="semibold" block style={{ flex: 1 }}>{group.name}</Text>
        <Button
          size="small"
          appearance="subtle"
          icon={<EditRegular />}
          aria-label="Rename group"
          onClick={openGroupRename}
        />
        <Text size={200} className={mutedClassName}>{peers.length} peer(s)</Text>
      </div>
      <div className={styles.peerList}>
        {peers.length === 0 ? (
          <div className={styles.empty}>
            <Text size={300}>No peers in this group yet.</Text>
          </div>
        ) : (
          peers.map((peer) => (
            <Card key={peer.id} className={styles.peerCard}>
              <div className={styles.peerHeader}>
                <div className={styles.peerTop}>
                  <Text weight="semibold">{peer.name}</Text>
                  <PeerStatusBadge enabled={peer.enabled} online={peer.online} />
                </div>
                <Button
                  size="small"
                  icon={<DeleteRegular />}
                  appearance="subtle"
                  aria-label="Delete peer"
                  onClick={() => onDeletePeer(peer.id, peer.name)}
                />
              </div>
              <div className={styles.peerMeta}>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>WireGuard IP</span>
                  <span className={styles.mono}>{peer.wg_ip}</span>
                </div>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>DNS</span>
                  <span className={styles.mono}>{peer.fqdn || `${peer.name}.${DNS_DOMAIN}`}</span>
                </div>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>Last handshake</span>
                  <Text size={200}>{formatHandshake(peer.last_handshake ?? 0)}</Text>
                </div>
                <div className={styles.metaItem}>
                  <span className={styles.metaLabel}>Traffic</span>
                  <Text size={200}>
                    {formatBytes(peer.rx_bytes ?? 0)} / {formatBytes(peer.tx_bytes ?? 0)}
                  </Text>
                </div>
              </div>
              <div className={styles.peerActions}>
                <Button className={styles.actionButton} size="small" icon={<EditRegular />} onClick={() => openPeerRename(peer.id, peer.name)}>
                  Rename
                </Button>
                <Button
                  className={styles.actionButton}
                  size="small"
                  icon={<PeopleTeamRegular />}
                  onClick={() => openMovePeer(peer.id, peer.name, peer.group_id)}
                >
                  Group
                </Button>
                <Button className={styles.actionButton} size="small" icon={<ArrowDownloadRegular />} onClick={() => onShowConfig(peer.id)}>
                  Config
                </Button>
                <Button className={styles.actionButton} size="small" icon={<PowerRegular />} onClick={() => onTogglePeer(peer.id)}>
                  Toggle
                </Button>
              </div>
            </Card>
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
      <RenameDialog
        open={peerRenameOpen}
        title="Rename peer"
        label="Hostname"
        value={peerRenameValue}
        error={peerRenameError}
        onValueChange={setPeerRenameValue}
        onClose={() => {
          setPeerRenameOpen(false);
          setPeerRenameId(null);
        }}
        onSave={() => void savePeerRename()}
      />

      <Dialog open={moveOpen} onOpenChange={(_, data) => !data.open && setMoveOpen(false)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Change group</DialogTitle>
            <DialogContent>
              <Field label="Peer">
                <Input value={movePeerName} readOnly />
              </Field>
              <Field label="Group">
                <Select value={moveGroupId} onChange={(_, data) => setMoveGroupId(data.value)}>
                  {groups.map((g) => (
                    <option key={g.id} value={String(g.id)}>{g.name}</option>
                  ))}
                </Select>
              </Field>
              {moveError && (
                <Text size={200} style={{ color: tokens.colorPaletteRedForeground1 }}>{moveError}</Text>
              )}
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setMoveOpen(false)}>Cancel</Button>
              <Button appearance="primary" onClick={() => void saveMovePeer()}>Save</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </aside>
  );
}
