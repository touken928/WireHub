import {
  Button,
  Card,
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
  DismissRegular,
  PeopleTeamRegular,
  PowerRegular,
  SearchRegular,
} from '@fluentui/react-icons';
import { useEffect, useMemo, useState } from 'react';
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
import {
  type PeerConnectionFilter,
  filterPeers,
  hasActivePeerFilters,
} from '@/pages/peers/filterPeers';
import { usePageLayoutStyles } from '@/styles/pageLayout';

const useStyles = makeStyles({
  toolbar: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: '10px',
    alignItems: 'flex-end',
  },
  searchField: {
    flex: '1 1 220px',
    minWidth: '200px',
  },
  filterField: {
    flex: '0 1 160px',
    minWidth: '140px',
  },
  resultHint: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  list: {
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  peerCard: {
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

export default function PeersPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { confirmDeletePeer } = useDestructiveConfirm();
  const peerConfig = usePeerConfig();

  const { peers, connected } = useStatus();
  const [groups, setGroups] = useState<PeerGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [groupFilter, setGroupFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<PeerConnectionFilter>('all');

  useEffect(() => {
    api.listGroups()
      .then((groupList) => {
        setGroups(groupList);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const filters = useMemo(
    () => ({
      query: searchQuery,
      groupId: groupFilter === '' ? null : Number(groupFilter),
      status: statusFilter,
    }),
    [searchQuery, groupFilter, statusFilter],
  );

  const filteredPeers = useMemo(() => filterPeers(peers, filters), [peers, filters]);
  const filtersActive = hasActivePeerFilters(filters);

  const handleDeletePeer = async (peer: PeerStatus) => {
    if (!(await confirmDeletePeer(peer.name))) return;
    await runPeerAction(async () => {
      await api.deletePeer(peer.id);
    });
  };

  const clearFilters = () => {
    setSearchQuery('');
    setGroupFilter('');
    setStatusFilter('all');
  };

  if (loading || !connected) return <Spinner label="Loading peers..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Peers"
        description="All peers with live status. Download config, toggle access, or delete from each row."
      />

      {peers.length === 0 ? (
        <div className={styles.empty}>
          <Text>No peers yet. Create a group and add peers from the Groups page.</Text>
        </div>
      ) : (
        <>
          <div className={styles.toolbar}>
            <Field label="Search" className={styles.searchField}>
              <Input
                value={searchQuery}
                placeholder="Name, DNS, IP, group…"
                contentBefore={<SearchRegular />}
                onChange={(_, data) => setSearchQuery(data.value)}
              />
            </Field>
            <Field label="Group" className={styles.filterField}>
              <Select
                value={groupFilter}
                onChange={(_, data) => setGroupFilter(data.value)}
              >
                <option value="">All groups</option>
                {groups.map((group) => (
                  <option key={group.id} value={String(group.id)}>{group.name}</option>
                ))}
              </Select>
            </Field>
            <Field label="Status" className={styles.filterField}>
              <Select
                value={statusFilter}
                onChange={(_, data) => setStatusFilter(data.value as PeerConnectionFilter)}
              >
                <option value="all">All statuses</option>
                <option value="online">Online</option>
                <option value="offline">Offline</option>
                <option value="disabled">Disabled</option>
              </Select>
            </Field>
            <Button
              appearance="subtle"
              icon={<DismissRegular />}
              disabled={!filtersActive}
              onClick={clearFilters}
            >
              Clear
            </Button>
          </div>

          <Text className={styles.resultHint}>
            Showing {filteredPeers.length} of {peers.length} peer{peers.length === 1 ? '' : 's'}
          </Text>

          {filteredPeers.length === 0 ? (
            <div className={styles.empty}>
              <Text>No peers match the current search or filters.</Text>
            </div>
          ) : (
            <div className={styles.list}>
              {filteredPeers.map((peer) => (
                <Card key={peer.id} className={styles.peerCard}>
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
                    <Button size="small" icon={<PowerRegular />} onClick={() => void api.togglePeer(peer.id)}>
                      Toggle
                    </Button>
                    <Button size="small" icon={<DeleteRegular />} appearance="subtle" onClick={() => void handleDeletePeer(peer)} />
                  </div>
                </Card>
              ))}
            </div>
          )}
        </>
      )}

      <ConfigDialog
        open={peerConfig.open}
        config={peerConfig.config}
        filename={peerConfig.filename}
        onClose={peerConfig.close}
      />
    </div>
  );
}
