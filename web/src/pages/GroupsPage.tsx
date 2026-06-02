import {
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react';
import type { Edge, Node } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
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
  Spinner,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import {
  ArrowDownloadRegular,
  DeleteRegular,
  PowerRegular,
} from '@fluentui/react-icons';
import {
  api,
  DNS_DOMAIN,
  formatBytes,
  formatHandshake,
} from '../api/client';
import type { GroupGraphNode, GroupGraphPeer, PeerStatus } from '../api/client';
import GroupsCanvas, {
  graphToFlow,
} from '../components/GroupsCanvas';
import type { GroupNodeData } from '../components/groupNodeTypes';
import ConfigDialog from '../components/ConfigDialog';
import PageHeader from '../components/PageHeader';
import { useDestructiveConfirm } from '../hooks/useDestructiveConfirm';
import { usePageLayoutStyles } from '../styles/pageLayout';

type EnrichedPeer = GroupGraphPeer & Partial<PeerStatus>;

const useStyles = makeStyles({
  workspace: {
    flex: '1 1 0',
    display: 'flex',
    gap: '16px',
    minHeight: 0,
    alignItems: 'stretch',
  },
  flow: {
    flex: '1 1 55%',
    minWidth: 0,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    overflow: 'hidden',
    backgroundColor: tokens.colorNeutralBackground1,
  },
  detailPanel: {
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
  detailPanelEmpty: {
    display: 'flex',
    gridTemplateRows: 'unset',
    alignItems: 'center',
    justifyContent: 'center',
  },
  detailHeader: {
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
    padding: '14px 16px',
    borderBottom: `1px solid ${tokens.colorNeutralStroke2}`,
  },
  memberList: {
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
  memberCard: {
    flexShrink: 0,
    padding: '14px',
    display: 'flex',
    flexDirection: 'column',
    gap: '10px',
    borderRadius: tokens.borderRadiusLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground2,
  },
  memberTop: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    flexWrap: 'wrap',
  },
  memberMeta: {
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
  memberActions: {
    display: 'flex',
    gap: '6px',
    flexWrap: 'wrap',
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
  panelEmpty: {
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
  contextMenu: {
    position: 'fixed',
    zIndex: 1000,
    backgroundColor: tokens.colorNeutralBackground1,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    borderRadius: tokens.borderRadiusMedium,
    boxShadow: tokens.shadow8,
    padding: '4px',
    display: 'flex',
    flexDirection: 'column',
    minWidth: '160px',
  },
});

function mergePeerStatus(peers: GroupGraphPeer[], status: PeerStatus[]): EnrichedPeer[] {
  const byId = new Map(status.map((p) => [p.id, p]));
  return peers.map((p) => ({ ...p, ...byId.get(p.id) }));
}

function pickLargestGroupId(groups: GroupGraphNode[]): number | null {
  if (groups.length === 0) return null;
  let best = groups[0];
  for (const g of groups) {
    const count = g.member_count ?? 0;
    const bestCount = best.member_count ?? 0;
    if (count > bestCount || (count === bestCount && g.id < best.id)) {
      best = g;
    }
  }
  return best.id;
}

export default function GroupsPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { confirmDeleteGroup, confirmDeletePeer, confirmDisconnectLinks } = useDestructiveConfirm();
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState('');
  const [flowNodes, setFlowNodes] = useState<Node<GroupNodeData>[]>([]);
  const [flowEdges, setFlowEdges] = useState<Edge[]>([]);
  const [graphRevision, setGraphRevision] = useState(0);
  const [graphGroups, setGraphGroups] = useState<GroupGraphNode[]>([]);
  const [peerStatus, setPeerStatus] = useState<PeerStatus[]>([]);
  const [createOpen, setCreateOpen] = useState(false);
  const [newGroupName, setNewGroupName] = useState('');
  const [detailGroupId, setDetailGroupId] = useState<number | null>(null);
  const [newUserName, setNewUserName] = useState('');
  const [createUserError, setCreateUserError] = useState('');
  const [edgeContextMenu, setEdgeContextMenu] = useState<{ x: number; y: number; edge: Edge } | null>(null);
  const [nodeContextMenu, setNodeContextMenu] = useState<{ x: number; y: number; groupId: number } | null>(null);
  const [configOpen, setConfigOpen] = useState(false);
  const [configText, setConfigText] = useState('');
  const [configFile, setConfigFile] = useState('peer.conf');

  const detailGroup = useMemo(
    () => graphGroups.find((g) => g.id === detailGroupId) ?? null,
    [graphGroups, detailGroupId],
  );

  const detailPeers = useMemo(
    () => (detailGroup ? mergePeerStatus(detailGroup.peers ?? [], peerStatus) : []),
    [detailGroup, peerStatus],
  );

  const load = useCallback(async () => {
    const [graph, status] = await Promise.all([api.getGroupGraph(), api.getStatus()]);
    const { nodes: n, edges: e, layoutPayload } = graphToFlow(graph, { autoLayout: true });
    setFlowNodes(n);
    setFlowEdges(e);
    if (layoutPayload.length > 0) {
      void api.updateGroupLayout(layoutPayload).catch(() => {});
    }
    const groups = graph.groups ?? [];
    setGraphGroups(groups);
    setPeerStatus(status.peers ?? []);
    setGraphRevision((v) => v + 1);
    setDetailGroupId((prev) => {
      if (groups.length === 0) return null;
      if (prev != null && groups.some((g) => g.id === prev)) return prev;
      return pickLargestGroupId(groups);
    });
    setLoadError('');
    setLoading(false);
  }, []);

  useEffect(() => {
    load().catch((err) => {
      setLoadError(err instanceof Error ? err.message : 'Failed to load groups');
      setLoading(false);
    });
    const t = setInterval(() => {
      api.getStatus().then((s) => setPeerStatus(s.peers ?? [])).catch(() => {});
    }, 5000);
    return () => clearInterval(t);
  }, [load]);

  useEffect(() => {
    const close = () => {
      setEdgeContextMenu(null);
      setNodeContextMenu(null);
    };
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, []);

  const onConnectLink = useCallback(async (from: number, to: number) => {
    await api.createGroupLink(from, to);
  }, []);

  const onDisconnectLinks = useCallback(async (toRemove: Edge[]) => {
    if (toRemove.length === 0) return;
    if (!(await confirmDisconnectLinks(toRemove.length))) return;
    for (const edge of toRemove) {
      await api.deleteGroupLink(Number(edge.source), Number(edge.target));
    }
    setEdgeContextMenu(null);
  }, [confirmDisconnectLinks]);

  const onLayoutChange = useCallback(async (nodes: Node<GroupNodeData>[]) => {
    await api.updateGroupLayout(
      nodes.map((n) => ({
        id: Number(n.id),
        pos_x: n.position.x,
        pos_y: n.position.y,
      })),
    );
  }, []);

  const onEdgeContextMenu = useCallback((event: React.MouseEvent, edge: Edge) => {
    event.preventDefault();
    setEdgeContextMenu({ x: event.clientX, y: event.clientY, edge });
    setNodeContextMenu(null);
  }, []);

  const selectGroup = (groupId: number) => {
    setDetailGroupId(groupId);
    setNewUserName('');
    setCreateUserError('');
  };

  const onNodeContextMenu = useCallback((event: React.MouseEvent, node: Node<GroupNodeData>) => {
    event.preventDefault();
    selectGroup(node.data.groupId);
    setNodeContextMenu({ x: event.clientX, y: event.clientY, groupId: node.data.groupId });
    setEdgeContextMenu(null);
  }, []);

  const refreshDetail = async () => {
    const graph = await api.getGroupGraph();
    setGraphGroups(graph.groups ?? []);
    const { nodes: n, edges: e } = graphToFlow(graph);
    setFlowNodes(n);
    setFlowEdges(e);
    setGraphRevision((v) => v + 1);
    const status = await api.getStatus();
    setPeerStatus(status.peers ?? []);
  };

  const handleCreateGroup = async () => {
    if (!newGroupName.trim()) return;
    await api.createGroup({ name: newGroupName.trim(), pos_x: 100, pos_y: 100 });
    setCreateOpen(false);
    setNewGroupName('');
    await load();
  };

  const handleCreateUserInGroup = async () => {
    if (!detailGroup || !newUserName.trim()) return;
    setCreateUserError('');
    try {
      await api.createPeer({ name: newUserName.trim(), group_id: detailGroup.id });
      setNewUserName('');
      await refreshDetail();
    } catch (e) {
      setCreateUserError(e instanceof Error ? e.message : 'Create failed');
    }
  };

  const handleDeleteGroup = async (groupId: number) => {
    if (!(await confirmDeleteGroup())) return;
    try {
      await api.deleteGroup(groupId);
      setNodeContextMenu(null);
      await load();
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  const handleDeletePeer = async (peerId: number, peerName: string) => {
    if (!(await confirmDeletePeer(peerName))) return;
    try {
      await api.deletePeer(peerId);
      await refreshDetail();
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  const showConfig = async (id: number) => {
    const { config, filename } = await api.getPeerConfig(id);
    setConfigText(config);
    setConfigFile(filename);
    setConfigOpen(true);
  };

  if (loading) return <Spinner label="Loading groups..." />;

  if (loadError) {
    return (
      <div className={`${pageLayout.page} ${pageLayout.pageFill}`}>
        <PageHeader title="Groups" />
        <Text style={{ color: tokens.colorPaletteRedForeground1 }}>{loadError}</Text>
        <Button onClick={() => { setLoading(true); load(); }}>Retry</Button>
      </div>
    );
  }

  return (
    <div className={`${pageLayout.page} ${pageLayout.pageFill}`}>
      <PageHeader
        title="Groups"
        description="Drag between groups to allow access. Click a group to manage members. Right-click a group or link for actions."
      />

      <div className={styles.workspace}>
        <div className={styles.flow}>
          <GroupsCanvas
            revision={graphRevision}
            initialNodes={flowNodes}
            initialEdges={flowEdges}
            selectedGroupId={detailGroupId}
            onConnectLink={onConnectLink}
            onDisconnectLinks={onDisconnectLinks}
            onLayoutChange={onLayoutChange}
            onEdgeContextMenu={onEdgeContextMenu}
            onNodeContextMenu={onNodeContextMenu}
            onNodeClick={(_, node) => selectGroup(node.data.groupId)}
            onDeleteGroup={(id) => void handleDeleteGroup(id)}
            onAddGroup={() => setCreateOpen(true)}
          />
        </div>

        <aside className={`${styles.detailPanel} ${!detailGroup ? styles.detailPanelEmpty : ''}`}>
          {detailGroup ? (
            <>
              <div className={styles.detailHeader}>
                <div>
                  <Text weight="semibold" block>{detailGroup.name}</Text>
                  <Text size={200} className={pageLayout.muted}>{detailPeers.length} member(s)</Text>
                </div>
              </div>
              <div className={styles.memberList}>
              {detailPeers.length === 0 ? (
                <div className={styles.empty}>
                  <Text size={300}>No users in this group yet.</Text>
                </div>
              ) : (
                detailPeers.map((p) => (
                  <Card key={p.id} className={styles.memberCard}>
                    <div className={styles.memberTop}>
                      <Text weight="semibold">{p.name}</Text>
                      <Badge
                          size="small"
                          appearance={p.enabled && p.online ? 'filled' : 'outline'}
                          color={!p.enabled ? 'danger' : p.online ? 'success' : 'informative'}
                        >
                          {!p.enabled ? 'Disabled' : p.online ? 'Online' : 'Offline'}
                      </Badge>
                    </div>
                    <div className={styles.memberMeta}>
                      <div className={styles.metaItem}>
                        <span className={styles.metaLabel}>WireGuard IP</span>
                        <span className={styles.mono}>{p.wg_ip}</span>
                      </div>
                      <div className={styles.metaItem}>
                        <span className={styles.metaLabel}>DNS</span>
                        <span className={styles.mono}>{p.fqdn || `${p.name}.${DNS_DOMAIN}`}</span>
                      </div>
                      <div className={styles.metaItem}>
                        <span className={styles.metaLabel}>Last handshake</span>
                        <Text size={200}>{formatHandshake(p.last_handshake ?? 0)}</Text>
                      </div>
                      <div className={styles.metaItem}>
                        <span className={styles.metaLabel}>Traffic</span>
                        <Text size={200}>
                          {formatBytes(p.rx_bytes ?? 0)} / {formatBytes(p.tx_bytes ?? 0)}
                        </Text>
                      </div>
                    </div>
                    <div className={styles.memberActions}>
                      <Button size="small" icon={<ArrowDownloadRegular />} onClick={() => showConfig(p.id)}>
                        Config
                      </Button>
                      <Button size="small" icon={<PowerRegular />} onClick={() => api.togglePeer(p.id).then(refreshDetail)}>
                        Toggle
                      </Button>
                      <Button
                        size="small"
                        icon={<DeleteRegular />}
                        appearance="subtle"
                        onClick={() => void handleDeletePeer(p.id, p.name)}
                      />
                    </div>
                  </Card>
                ))
              )}
              </div>
              <div className={styles.addSection}>
                <Text weight="semibold" size={300}>Add user</Text>
                <div className={styles.addRow}>
                  <Field style={{ flex: 1 }}>
                    <Input
                      value={newUserName}
                      placeholder="alice"
                      onChange={(_, d) => setNewUserName(d.value)}
                    />
                  </Field>
                  <Button appearance="primary" onClick={handleCreateUserInGroup} disabled={!newUserName.trim()}>
                    Add
                  </Button>
                </div>
                {createUserError && (
                  <Text size={200} style={{ color: tokens.colorPaletteRedForeground1 }}>{createUserError}</Text>
                )}
              </div>
            </>
          ) : (
            <div className={styles.panelEmpty}>
              <Text size={300}>No groups yet. Add a group to get started.</Text>
            </div>
          )}
        </aside>
      </div>

      {edgeContextMenu && (
        <div
          className={styles.contextMenu}
          style={{ left: edgeContextMenu.x, top: edgeContextMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <Button appearance="subtle" onClick={() => onDisconnectLinks([edgeContextMenu.edge]).then(load)}>
            Disconnect
          </Button>
        </div>
      )}

      {nodeContextMenu && (
        <div
          className={styles.contextMenu}
          style={{ left: nodeContextMenu.x, top: nodeContextMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <Button
            appearance="subtle"
            icon={<DeleteRegular />}
            onClick={() => void handleDeleteGroup(nodeContextMenu.groupId)}
          >
            Delete group
          </Button>
        </div>
      )}

      <Dialog open={createOpen} onOpenChange={(_, d) => setCreateOpen(d.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>New group</DialogTitle>
            <DialogContent>
              <Field label="Name" required>
                <Input value={newGroupName} onChange={(_, d) => setNewGroupName(d.value)} />
              </Field>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setCreateOpen(false)}>Cancel</Button>
              <Button appearance="primary" onClick={handleCreateGroup}>Create</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <ConfigDialog open={configOpen} config={configText} filename={configFile} onClose={() => setConfigOpen(false)} />
    </div>
  );
}
