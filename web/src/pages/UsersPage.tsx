import {
  Badge,
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
import { useCallback, useEffect, useState } from 'react';
import {
  api,
  formatBytes,
  formatHandshake,
  DNS_DOMAIN,
} from '../api/client';
import type { PeerGroup, PeerStatus } from '../api/client';
import ConfigDialog from '../components/ConfigDialog';
import PageHeader from '../components/PageHeader';
import { useDestructiveConfirm } from '../hooks/useDestructiveConfirm';
import { usePageLayoutStyles } from '../styles/pageLayout';

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
  const [peers, setPeers] = useState<PeerStatus[]>([]);
  const [groups, setGroups] = useState<PeerGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [configOpen, setConfigOpen] = useState(false);
  const [configText, setConfigText] = useState('');
  const [configFile, setConfigFile] = useState('peer.conf');
  const [moveOpen, setMoveOpen] = useState(false);
  const [movePeer, setMovePeer] = useState<PeerStatus | null>(null);
  const [moveGroupId, setMoveGroupId] = useState('');

  const load = useCallback(() => {
    Promise.all([api.getStatus(), api.listGroups()])
      .then(([status, groupList]) => {
        setPeers(status.peers ?? []);
        setGroups(groupList);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, [load]);

  const showConfig = async (id: number) => {
    const { config, filename } = await api.getPeerConfig(id);
    setConfigText(config);
    setConfigFile(filename);
    setConfigOpen(true);
  };

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
    load();
  };

  const handleDeletePeer = async (peer: PeerStatus) => {
    if (!(await confirmDeletePeer(peer.name))) return;
    try {
      await api.deletePeer(peer.id);
      load();
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  if (loading) return <Spinner label="Loading users..." />;

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
          {peers.map((p) => (
              <Card key={p.id} className={styles.userCard}>
                <div className={styles.identity}>
                  <div className={styles.nameRow}>
                    <Text weight="semibold">{p.name}</Text>
                    <Badge
                      size="small"
                      appearance={p.enabled && p.online ? 'filled' : 'outline'}
                      color={!p.enabled ? 'danger' : p.online ? 'success' : 'informative'}
                    >
                      {!p.enabled ? 'Disabled' : p.online ? 'Online' : 'Offline'}
                    </Badge>
                  </div>
                  <span className={styles.groupTag}>
                    <PeopleTeamRegular fontSize={14} />
                    {p.group_name || '—'}
                  </span>
                </div>
                <div className={styles.stat}>
                  <span className={styles.statLabel}>WireGuard IP</span>
                  <span className={`${styles.statValue} ${styles.mono}`}>{p.wg_ip}</span>
                </div>
                <div className={styles.stat}>
                  <span className={styles.statLabel}>DNS</span>
                  <span className={`${styles.statValue} ${styles.mono}`}>{p.fqdn || `${p.name}.${DNS_DOMAIN}`}</span>
                </div>
                <div className={styles.stat}>
                  <span className={styles.statLabel}>Last handshake</span>
                  <span className={styles.statValue}>{formatHandshake(p.last_handshake)}</span>
                </div>
                <div className={styles.stat}>
                  <span className={styles.statLabel}>Traffic</span>
                  <span className={styles.statValue}>{formatBytes(p.rx_bytes)} / {formatBytes(p.tx_bytes)}</span>
                </div>
                <div className={styles.actions}>
                  <Button size="small" icon={<ArrowDownloadRegular />} onClick={() => showConfig(p.id)}>
                    Config
                  </Button>
                  <Button size="small" onClick={() => openMove(p)}>Group</Button>
                  <Button size="small" icon={<PowerRegular />} onClick={() => api.togglePeer(p.id).then(load)}>
                    Toggle
                  </Button>
                  <Button size="small" icon={<DeleteRegular />} appearance="subtle" onClick={() => void handleDeletePeer(p)} />
                </div>
              </Card>
            ))}
        </div>
      )}

      <ConfigDialog open={configOpen} config={configText} filename={configFile} onClose={() => setConfigOpen(false)} />

      <Dialog open={moveOpen} onOpenChange={(_, d) => setMoveOpen(d.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Change group</DialogTitle>
            <DialogContent>
              <Field label="User">
                <Input value={movePeer?.name ?? ''} readOnly />
              </Field>
              <Field label="Group">
                <Select value={moveGroupId} onChange={(_, d) => setMoveGroupId(d.value)}>
                  {groups.map((g) => (
                    <option key={g.id} value={String(g.id)}>{g.name}</option>
                  ))}
                </Select>
              </Field>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setMoveOpen(false)}>Cancel</Button>
              <Button appearance="primary" onClick={handleMove}>Save</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
