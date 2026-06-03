import {
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react';
import type { Edge, Node } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import {
  Button,
  Spinner,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { DeleteRegular } from '@fluentui/react-icons';
import { useStatus } from '@/app/StatusProvider';
import { api } from '@/api';
import type { GroupGraphNode } from '@/api/types';
import GroupsCanvas from '@/components/groups/GroupsCanvas';
import { edgeLinkEndpoints, graphToFlow } from '@/components/groups/groupGraph';
import type { GroupNodeData } from '@/components/groups/types';
import { ContextMenu } from '@/components/groups/ContextMenu';
import { CreateGroupDialog } from '@/components/groups/CreateGroupDialog';
import { GroupDetailPanel } from '@/components/groups/GroupDetailPanel';
import { ConfigDialog } from '@/components/peers/ConfigDialog';
import { PageHeader } from '@/components/layout/PageHeader';
import { useDestructiveConfirm } from '@/hooks/useDestructiveConfirm';
import { usePeerConfig, runPeerAction } from '@/hooks/usePeerConfig';
import { getErrorMessage } from '@/lib/error';
import { mergePeerStatus, pickLargestGroupId } from '@/pages/groups/utils';
import { usePageLayoutStyles } from '@/styles/pageLayout';

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
});

export default function GroupsPage() {
  const styles = useStyles();
  const pageLayout = usePageLayoutStyles();
  const { confirmDeleteGroup, confirmDeletePeer, confirmDisconnectLinks } = useDestructiveConfirm();
  const peerConfig = usePeerConfig();

  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState('');
  const [flowNodes, setFlowNodes] = useState<Node<GroupNodeData>[]>([]);
  const [flowEdges, setFlowEdges] = useState<Edge[]>([]);
  const [graphRevision, setGraphRevision] = useState(0);
  const [graphGroups, setGraphGroups] = useState<GroupGraphNode[]>([]);
  const { peers: peerStatus } = useStatus();
  const [createOpen, setCreateOpen] = useState(false);
  const [newGroupName, setNewGroupName] = useState('');
  const [detailGroupId, setDetailGroupId] = useState<number | null>(null);
  const [newUserName, setNewUserName] = useState('');
  const [createUserError, setCreateUserError] = useState('');
  const [nodeContextMenu, setNodeContextMenu] = useState<{ x: number; y: number; groupId: number } | null>(null);

  const detailGroup = useMemo(
    () => graphGroups.find((g) => g.id === detailGroupId) ?? null,
    [graphGroups, detailGroupId],
  );

  const detailPeers = useMemo(
    () => (detailGroup ? mergePeerStatus(detailGroup.peers ?? [], peerStatus) : []),
    [detailGroup, peerStatus],
  );

  const load = useCallback(async () => {
    const graph = await api.getGroupGraph();
    const { nodes, edges, layoutPayload } = graphToFlow(graph, { autoLayout: true });
    setFlowNodes(nodes);
    setFlowEdges(edges);
    if (layoutPayload.length > 0) {
      void api.updateGroupLayout(layoutPayload).catch(() => {});
    }
    const groups = graph.groups ?? [];
    setGraphGroups(groups);
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
    let cancelled = false;
    void (async () => {
      try {
        await load();
      } catch (err) {
        if (!cancelled) {
          setLoadError(getErrorMessage(err, 'Failed to load groups'));
          setLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [load]);

  useEffect(() => {
    const close = () => setNodeContextMenu(null);
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, []);

  const selectGroup = (groupId: number) => {
    setDetailGroupId(groupId);
    setNewUserName('');
    setCreateUserError('');
  };

  const refreshDetail = async () => {
    const graph = await api.getGroupGraph();
    setGraphGroups(graph.groups ?? []);
    const { nodes, edges } = graphToFlow(graph);
    setFlowNodes(nodes);
    setFlowEdges(edges);
    setGraphRevision((v) => v + 1);
  };

  const onConnectLink = useCallback(async (from: number, to: number, bidirectional: boolean) => {
    await api.createGroupLink({ from_group_id: from, to_group_id: to, bidirectional });
  }, []);

  const onDisconnectLinks = useCallback(async (toRemove: Edge[]) => {
    if (toRemove.length === 0) return;
    if (!(await confirmDisconnectLinks(toRemove.length))) return;
    for (const edge of toRemove) {
      const { from, to } = edgeLinkEndpoints(edge);
      await api.deleteGroupLink(from, to);
    }
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

  const onNodeContextMenu = useCallback((event: React.MouseEvent, node: Node<GroupNodeData>) => {
    event.preventDefault();
    selectGroup(node.data.groupId);
    setNodeContextMenu({ x: event.clientX, y: event.clientY, groupId: node.data.groupId });
  }, []);

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
    } catch (err) {
      setCreateUserError(getErrorMessage(err, 'Create failed'));
    }
  };

  const handleDeleteGroup = async (groupId: number) => {
    if (!(await confirmDeleteGroup())) return;
    await runPeerAction(async () => {
      await api.deleteGroup(groupId);
      setNodeContextMenu(null);
      await load();
    });
  };

  const handleDeletePeer = async (peerId: number, peerName: string) => {
    if (!(await confirmDeletePeer(peerName))) return;
    await runPeerAction(async () => {
      await api.deletePeer(peerId);
      await refreshDetail();
    });
  };

  if (loading) return <Spinner label="Loading groups..." />;

  if (loadError) {
    return (
      <div className={`${pageLayout.page} ${pageLayout.pageFill}`}>
        <PageHeader title="Groups" />
        <Text style={{ color: tokens.colorPaletteRedForeground1 }}>{loadError}</Text>
        <Button onClick={() => { setLoading(true); void load(); }}>Retry</Button>
      </div>
    );
  }

  return (
    <div className={`${pageLayout.page} ${pageLayout.pageFill}`}>
      <PageHeader
        title="Groups"
        description="Drag between groups to connect. Use the switch at bottom-left for one-way links."
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
            onNodeContextMenu={onNodeContextMenu}
            onNodeClick={(_, node) => selectGroup(node.data.groupId)}
            onDeleteGroup={(id) => void handleDeleteGroup(id)}
            onAddGroup={() => setCreateOpen(true)}
          />
        </div>

        <GroupDetailPanel
          group={detailGroup}
          peers={detailPeers}
          mutedClassName={pageLayout.muted}
          newUserName={newUserName}
          createUserError={createUserError}
          onNewUserNameChange={setNewUserName}
          onCreateUser={() => void handleCreateUserInGroup()}
          onShowConfig={(id) => void peerConfig.showConfig(id)}
          onTogglePeer={(id) => void api.togglePeer(id).then(refreshDetail)}
          onDeletePeer={(id, name) => void handleDeletePeer(id, name)}
        />
      </div>

      {nodeContextMenu && (
        <ContextMenu x={nodeContextMenu.x} y={nodeContextMenu.y}>
          <Button
            appearance="subtle"
            icon={<DeleteRegular />}
            onClick={() => void handleDeleteGroup(nodeContextMenu.groupId)}
          >
            Delete group
          </Button>
        </ContextMenu>
      )}

      <CreateGroupDialog
        open={createOpen}
        name={newGroupName}
        onNameChange={setNewGroupName}
        onClose={() => setCreateOpen(false)}
        onCreate={() => void handleCreateGroup()}
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
