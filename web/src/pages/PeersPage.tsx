import {
  Button,
  Field,
  Input,
  Select,
  Spinner,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { AddRegular, DismissRegular, SearchRegular } from '@fluentui/react-icons';
import { useEffect, useMemo, useState } from 'react';
import { useStatus } from '@/app/StatusProvider';
import { api } from '@/api';
import type { PeerGroup, PeerStatus } from '@/api/types';
import { ConfigDialog } from '@/components/peers/ConfigDialog';
import { CreatePeerDialog } from '@/components/peers/CreatePeerDialog';
import { PeerMemberCard, type PeerMemberCardGroup } from '@/components/peers/PeerMemberCard';
import { PageHeader } from '@/components/layout/PageHeader';
import { useDestructiveConfirm } from '@/hooks/useDestructiveConfirm';
import { usePeerConfig, runPeerAction } from '@/hooks/usePeerConfig';
import { getErrorMessage } from '@/lib/error';
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
  toolbarActions: {
    marginLeft: 'auto',
    display: 'flex',
    alignItems: 'flex-end',
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
  empty: {
    padding: '32px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px dashed ${tokens.colorNeutralStroke2}`,
  },
});

function toMemberCardPeer(peer: PeerStatus) {
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
  const [createOpen, setCreateOpen] = useState(false);
  const [newPeerName, setNewPeerName] = useState('');
  const [newPeerGroupId, setNewPeerGroupId] = useState('');
  const [createPeerError, setCreatePeerError] = useState('');

  useEffect(() => {
    api.listGroups()
      .then((groupList) => {
        setGroups(groupList);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const groupOptions = useMemo<PeerMemberCardGroup[]>(
    () => groups.map((g) => ({ id: g.id, name: g.name })),
    [groups],
  );

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

  const handleDeletePeer = async (peerId: number, peerName: string) => {
    if (!(await confirmDeletePeer(peerName))) return;
    await runPeerAction(async () => {
      await api.deletePeer(peerId);
    });
  };

  const clearFilters = () => {
    setSearchQuery('');
    setGroupFilter('');
    setStatusFilter('all');
  };

  const defaultCreateGroupId = () => {
    if (groupFilter) return groupFilter;
    if (groups.length === 0) return '';
    let best = groups[0];
    for (const group of groups) {
      if (group.member_count > best.member_count) best = group;
    }
    return String(best.id);
  };

  const openCreateDialog = () => {
    setCreatePeerError('');
    setNewPeerName('');
    setNewPeerGroupId(defaultCreateGroupId());
    setCreateOpen(true);
  };

  const handleCreatePeer = async () => {
    if (!newPeerName.trim() || !newPeerGroupId) return;
    setCreatePeerError('');
    try {
      await api.createPeer({
        name: newPeerName.trim(),
        group_id: Number(newPeerGroupId),
      });
      setCreateOpen(false);
      setNewPeerName('');
    } catch (err) {
      setCreatePeerError(getErrorMessage(err, 'Create failed'));
    }
  };

  if (loading || !connected) return <Spinner label="Loading peers..." />;

  return (
    <div className={pageLayout.page}>
      <PageHeader
        title="Peers"
        description="All peers with live status. Create peers, rename, change group, download config, or toggle access from each row."
        actions={(
          <Button appearance="primary" icon={<AddRegular />} onClick={openCreateDialog}>
            Add peer
          </Button>
        )}
      />

      {groups.length === 0 ? (
        <div className={styles.empty}>
          <Text>No groups yet. Create a group on the Groups page before adding peers.</Text>
        </div>
      ) : (
        <>
          {peers.length > 0 ? (
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
          ) : null}

          {peers.length === 0 ? (
            <div className={styles.empty}>
              <Text>No peers yet. Click Add peer to create one.</Text>
            </div>
          ) : (
            <>
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
                    <PeerMemberCard
                      key={peer.id}
                      layout="row"
                      peer={toMemberCardPeer(peer)}
                      groups={groupOptions}
                      showGroupTag
                      onRename={async (id, name) => { await api.updatePeer(id, { name }); }}
                      onMove={async (id, groupId) => { await api.updatePeer(id, { group_id: groupId }); }}
                      onShowConfig={(id) => void peerConfig.showConfig(id)}
                      onToggle={(id) => void api.togglePeer(id)}
                      onDelete={(id, name) => void handleDeletePeer(id, name)}
                    />
                  ))}
                </div>
              )}
            </>
          )}
        </>
      )}

      <CreatePeerDialog
        open={createOpen}
        name={newPeerName}
        groupId={newPeerGroupId}
        groups={groupOptions}
        error={createPeerError}
        onNameChange={setNewPeerName}
        onGroupChange={setNewPeerGroupId}
        onClose={() => setCreateOpen(false)}
        onCreate={() => void handleCreatePeer()}
      />

      <ConfigDialog
        open={peerConfig.open}
        config={peerConfig.config}
        filename={peerConfig.filename}
        onClose={peerConfig.close}
      />
    </div>
  );
}
