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
import { DNS_DOMAIN } from '@/constants';
import { RenameDialog } from '@/components/common/RenameDialog';
import { PeerStatusBadge } from '@/components/peers/PeerStatusBadge';
import { formatBytes, formatHandshake } from '@/lib/format';
import { getErrorMessage } from '@/lib/error';

const useStyles = makeStyles({
  card: {
    flexShrink: 0,
    padding: '14px',
    display: 'flex',
    flexDirection: 'column',
    gap: '10px',
    borderRadius: tokens.borderRadiusLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground2,
  },
  header: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: '8px',
  },
  headerActions: {
    display: 'flex',
    alignItems: 'center',
    gap: '2px',
    flexShrink: 0,
  },
  identity: {
    flex: 1,
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: '6px',
  },
  nameRow: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    flexWrap: 'wrap',
  },
  groupTag: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '4px',
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  meta: {
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
  actions: {
    display: 'grid',
    gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
    gap: '6px',
  },
  actionButton: {
    width: '100%',
    minWidth: 0,
    paddingLeft: '6px',
    paddingRight: '6px',
  },
  rowCard: {
    padding: '16px 18px',
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow2,
    display: 'grid',
    gridTemplateColumns: 'minmax(160px, 1.2fr) repeat(4, minmax(0, 1fr)) auto',
    gap: '12px 16px',
    alignItems: 'center',
    '@media (max-width: 960px)': {
      gridTemplateColumns: '1fr',
    },
  },
  rowIdentity: {
    display: 'flex',
    flexDirection: 'column',
    gap: '6px',
    minWidth: 0,
  },
  rowStat: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
    minWidth: 0,
  },
  rowStatValue: {
    fontSize: tokens.fontSizeBase300,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  rowActions: {
    display: 'flex',
    gap: '6px',
    flexWrap: 'wrap',
    justifyContent: 'flex-end',
    alignItems: 'center',
  },
});

export type PeerMemberCardGroup = {
  id: number;
  name: string;
};

export type PeerMemberCardPeer = {
  id: number;
  name: string;
  fqdn: string;
  wg_ip: string;
  group_id: number;
  group_name?: string;
  enabled: boolean;
  online?: boolean;
  last_handshake: number;
  rx_bytes: number;
  tx_bytes: number;
};

type PeerMemberCardProps = {
  peer: PeerMemberCardPeer;
  groups: PeerMemberCardGroup[];
  layout?: 'card' | 'row';
  showGroupTag?: boolean;
  onRename: (peerId: number, name: string) => Promise<void>;
  onMove: (peerId: number, groupId: number) => Promise<void>;
  onShowConfig: (peerId: number) => void;
  onToggle: (peerId: number) => void;
  onDelete: (peerId: number, peerName: string) => void;
};

export function PeerMemberCard({
  peer,
  groups,
  layout = 'card',
  showGroupTag = false,
  onRename,
  onMove,
  onShowConfig,
  onToggle,
  onDelete,
}: PeerMemberCardProps) {
  const styles = useStyles();
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState('');
  const [renameError, setRenameError] = useState('');
  const [moveOpen, setMoveOpen] = useState(false);
  const [moveGroupId, setMoveGroupId] = useState('');
  const [moveError, setMoveError] = useState('');

  const openRename = () => {
    setRenameValue(peer.name);
    setRenameError('');
    setRenameOpen(true);
  };

  const saveRename = async () => {
    const name = renameValue.trim();
    if (!name) return;
    setRenameError('');
    try {
      await onRename(peer.id, name);
      setRenameOpen(false);
    } catch (err) {
      setRenameError(getErrorMessage(err, 'Rename failed'));
    }
  };

  const openMove = () => {
    setMoveGroupId(String(peer.group_id));
    setMoveError('');
    setMoveOpen(true);
  };

  const saveMove = async () => {
    if (!moveGroupId) return;
    setMoveError('');
    try {
      await onMove(peer.id, Number(moveGroupId));
      setMoveOpen(false);
    } catch (err) {
      setMoveError(getErrorMessage(err, 'Move failed'));
    }
  };

  return (
    <>
      {layout === 'row' ? (
        <Card className={styles.rowCard}>
          <div className={styles.rowIdentity}>
            <div className={styles.nameRow}>
              <Text weight="semibold">{peer.name}</Text>
              <PeerStatusBadge enabled={peer.enabled} online={peer.online} />
            </div>
            {showGroupTag && (
              <span className={styles.groupTag}>
                <PeopleTeamRegular fontSize={14} />
                {peer.group_name || '—'}
              </span>
            )}
          </div>
          <div className={styles.rowStat}>
            <span className={styles.metaLabel}>WireGuard IP</span>
            <span className={`${styles.rowStatValue} ${styles.mono}`}>{peer.wg_ip}</span>
          </div>
          <div className={styles.rowStat}>
            <span className={styles.metaLabel}>DNS</span>
            <span className={`${styles.rowStatValue} ${styles.mono}`}>{peer.fqdn || `${peer.name}.${DNS_DOMAIN}`}</span>
          </div>
          <div className={styles.rowStat}>
            <span className={styles.metaLabel}>Last handshake</span>
            <span className={styles.rowStatValue}>{formatHandshake(peer.last_handshake)}</span>
          </div>
          <div className={styles.rowStat}>
            <span className={styles.metaLabel}>Traffic</span>
            <span className={styles.rowStatValue}>{formatBytes(peer.rx_bytes)} / {formatBytes(peer.tx_bytes)}</span>
          </div>
          <div className={styles.rowActions}>
            <Button size="small" icon={<PeopleTeamRegular />} onClick={openMove}>Group</Button>
            <Button size="small" icon={<ArrowDownloadRegular />} onClick={() => onShowConfig(peer.id)}>Config</Button>
            <Button size="small" icon={<PowerRegular />} onClick={() => onToggle(peer.id)}>Toggle</Button>
            <Button size="small" icon={<EditRegular />} appearance="subtle" aria-label="Rename peer" onClick={openRename} />
            <Button size="small" icon={<DeleteRegular />} appearance="subtle" aria-label="Delete peer" onClick={() => onDelete(peer.id, peer.name)} />
          </div>
        </Card>
      ) : (
        <Card className={styles.card}>
          <div className={styles.header}>
            <div className={styles.identity}>
              <div className={styles.nameRow}>
                <Text weight="semibold">{peer.name}</Text>
                <PeerStatusBadge enabled={peer.enabled} online={peer.online} />
              </div>
              {showGroupTag && (
                <span className={styles.groupTag}>
                  <PeopleTeamRegular fontSize={14} />
                  {peer.group_name || '—'}
                </span>
              )}
            </div>
            <div className={styles.headerActions}>
              <Button
                size="small"
                icon={<EditRegular />}
                appearance="subtle"
                aria-label="Rename peer"
                onClick={openRename}
              />
              <Button
                size="small"
                icon={<DeleteRegular />}
                appearance="subtle"
                aria-label="Delete peer"
                onClick={() => onDelete(peer.id, peer.name)}
              />
            </div>
          </div>
          <div className={styles.meta}>
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
              <Text size={200}>{formatHandshake(peer.last_handshake)}</Text>
            </div>
            <div className={styles.metaItem}>
              <span className={styles.metaLabel}>Traffic</span>
              <Text size={200}>
                {formatBytes(peer.rx_bytes)} / {formatBytes(peer.tx_bytes)}
              </Text>
            </div>
          </div>
          <div className={styles.actions}>
            <Button className={styles.actionButton} size="small" icon={<PeopleTeamRegular />} onClick={openMove}>
              Group
            </Button>
            <Button className={styles.actionButton} size="small" icon={<ArrowDownloadRegular />} onClick={() => onShowConfig(peer.id)}>
              Config
            </Button>
            <Button className={styles.actionButton} size="small" icon={<PowerRegular />} onClick={() => onToggle(peer.id)}>
              Toggle
            </Button>
          </div>
        </Card>
      )}

      <RenameDialog
        open={renameOpen}
        title="Rename peer"
        label="Hostname"
        value={renameValue}
        error={renameError}
        onValueChange={setRenameValue}
        onClose={() => setRenameOpen(false)}
        onSave={() => void saveRename()}
      />

      <Dialog open={moveOpen} onOpenChange={(_, data) => !data.open && setMoveOpen(false)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Change group</DialogTitle>
            <DialogContent>
              <Field label="Peer">
                <Input value={peer.name} readOnly />
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
              <Button appearance="primary" onClick={() => void saveMove()}>Save</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </>
  );
}
