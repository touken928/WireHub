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
  Spinner,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import {
  ArrowDownloadRegular,
  DeleteRegular,
  PeopleTeamRegular,
  PowerRegular,
} from '@fluentui/react-icons';
import { useEffect, useState } from 'react';
import { useStatus } from '@/app/StatusProvider';
import {
  api,
  formatBytes,
  formatHandshake,
} from '@/api';
import type { PeerGroup, PeerStatus } from '@/api/types';
import { DNS_DOMAIN } from '@/constants';
import { ConfigDialog } from '@/components/peers/ConfigDialog';
import { PeerStatusBadge } from '@/components/peers/PeerStatusBadge';
import { PageHeader } from '@/components/layout/PageHeader';
import { useDestructiveConfirm } from '@/hooks/useDestructiveConfirm';
import { usePeerConfig, runPeerAction } from '@/hooks/usePeerConfig';
import { usePageLayoutStyles } from '@/styles/pageLayout';

const useStyles = makeStyles({
  list: {
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  userCard: {
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
  identity: {
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
  stat: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
    minWidth: 0,
  },
  statLabel: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  statValue: {
    fontSize: tokens.fontSizeBase300,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
  },
  actions: {
    display: 'flex',
    gap: '6px',
    flexWrap: 'wrap',
    justifyContent: 'flex-end',
  },
  empty: {
    padding: '32px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px dashed ${tokens.colorNeutralStroke2}`,
  },
});

export default function UsersPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { confirmDeletePeer } = useDestructiveConfirm();
  const peerConfig = usePeerConfig();

  const { peers, connected } = useStatus();
  const [groups, setGroups] = useState<PeerGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [moveOpen, setMoveOpen] = useState(false);
  const [movePeer, setMovePeer] = useState<PeerStatus | null>(null);
  const [moveGroupId, setMoveGroupId] = useState('');

  useEffect(() => {
    api.listGroups()
      .then((groupList) => {
        setGroups(groupList);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const openMove = (peer: PeerStatus) => {
    setMovePeer(peer);
    setMoveGroupId(String(peer.group_id));
    setMoveOpen(true);
  };

  const handleMove = async () => {
    if (!movePeer) return;
    await api.updatePeer(movePeer.id, { group_id: Number(moveGroupId) });
    setMoveOpen(false);
    setMovePeer(null);
  };

  const handleDeletePeer = async (peer: PeerStatus) => {
    if (!(await confirmDeletePeer(peer.name))) return;
    await runPeerAction(async () => {
      await api.deletePeer(peer.id);
    });
  };

  if (loading || !connected) return <Spinner label="Loading users..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Users"
        description="All peers with live status. Manage config, group membership, and access from each row."
      />

      {peers.length === 0 ? (
        <div className={styles.empty}>
          <Text>No users yet. Create a group and add users from the Groups page.</Text>
        </div>
      ) : (
        <div className={styles.list}>
          {peers.map((peer) => (
            <Card key={peer.id} className={styles.userCard}>
              <div className={styles.identity}>
                <div className={styles.nameRow}>
                  <Text weight="semibold">{peer.name}</Text>
                  <PeerStatusBadge enabled={peer.enabled} online={peer.online} />
                </div>
                <span className={styles.groupTag}>
                  <PeopleTeamRegular fontSize={14} />
                  {peer.group_name || '—'}
                </span>
              </div>
              <div className={styles.stat}>
                <span className={styles.statLabel}>WireGuard IP</span>
                <span className={`${styles.statValue} ${styles.mono}`}>{peer.wg_ip}</span>
              </div>
              <div className={styles.stat}>
                <span className={styles.statLabel}>DNS</span>
                <span className={`${styles.statValue} ${styles.mono}`}>{peer.fqdn || `${peer.name}.${DNS_DOMAIN}`}</span>
              </div>
              <div className={styles.stat}>
                <span className={styles.statLabel}>Last handshake</span>
                <span className={styles.statValue}>{formatHandshake(peer.last_handshake)}</span>
              </div>
              <div className={styles.stat}>
                <span className={styles.statLabel}>Traffic</span>
                <span className={styles.statValue}>{formatBytes(peer.rx_bytes)} / {formatBytes(peer.tx_bytes)}</span>
              </div>
              <div className={styles.actions}>
                <Button size="small" icon={<ArrowDownloadRegular />} onClick={() => void peerConfig.showConfig(peer.id)}>
                  Config
                </Button>
                <Button size="small" onClick={() => openMove(peer)}>Group</Button>
                <Button size="small" icon={<PowerRegular />} onClick={() => void api.togglePeer(peer.id)}>
                  Toggle
                </Button>
                <Button size="small" icon={<DeleteRegular />} appearance="subtle" onClick={() => void handleDeletePeer(peer)} />
              </div>
            </Card>
          ))}
        </div>
      )}

      <ConfigDialog
        open={peerConfig.open}
        config={peerConfig.config}
        filename={peerConfig.filename}
        onClose={peerConfig.close}
      />

      <Dialog open={moveOpen} onOpenChange={(_, data) => setMoveOpen(data.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Change group</DialogTitle>
            <DialogContent>
              <Field label="User">
                <Input value={movePeer?.name ?? ''} readOnly />
              </Field>
              <Field label="Group">
                <Select value={moveGroupId} onChange={(_, data) => setMoveGroupId(data.value)}>
                  {groups.map((group) => (
                    <option key={group.id} value={String(group.id)}>{group.name}</option>
                  ))}
                </Select>
              </Field>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setMoveOpen(false)}>Cancel</Button>
              <Button appearance="primary" onClick={() => void handleMove()}>Save</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
