import {
  useCallback,
  useEffect,
  useState,
} from 'react';
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  useNodesState,
  useEdgesState,
  useReactFlow,
  type Connection,
  type Edge,
  type Node,
  type EdgeTypes,
  type NodeTypes,
  type OnBeforeDelete,
  ConnectionMode,
  Panel,
} from '@xyflow/react';
import { Button } from '@fluentui/react-components';
import { AddRegular, DeleteRegular, DismissRegular } from '@fluentui/react-icons';
import GroupNode from '@/components/groups/GroupNode';
import { LinkDrawToolbar } from '@/components/groups/LinkDrawToolbar';
import type { GroupNodeData, LinkDrawMode } from '@/components/groups/types';
import {
  appendGroupEdge,
  connectionLinkEnds,
  hasGroupLink,
  linkEdgeId,
} from '@/components/groups/groupGraph';
import GroupLinkEdge from '@/components/groups/GroupLinkEdge';
import { rematchEdgesForNode } from '@/components/groups/groupLayout';
import { getErrorMessage } from '@/lib/error';

const nodeTypes: NodeTypes = {
  peerGroup: GroupNode,
};

const edgeTypes: EdgeTypes = {
  groupLink: GroupLinkEdge,
};

const defaultEdgeOptions = {
  deletable: true,
  selectable: true,
  interactionWidth: 24,
};

type GroupsCanvasProps = {
  revision: number;
  initialNodes: Node<GroupNodeData>[];
  initialEdges: Edge[];
  selectedGroupId: number | null;
  onConnectLink: (from: number, to: number, bidirectional: boolean) => Promise<void>;
  onDisconnectLinks: (edges: Edge[]) => Promise<void>;
  onLayoutChange: (nodes: Node<GroupNodeData>[]) => Promise<void>;
  onNodeContextMenu: (event: React.MouseEvent, node: Node<GroupNodeData>) => void;
  onNodeClick: (event: React.MouseEvent, node: Node<GroupNodeData>) => void;
  onDeleteGroup: (groupId: number) => void;
  onAddGroup: () => void;
};

function FitViewOnMount() {
  const { fitView } = useReactFlow();
  useEffect(() => {
    const id = requestAnimationFrame(() => {
      fitView({ padding: 0.2, maxZoom: 1.5, duration: 200 });
    });
    return () => cancelAnimationFrame(id);
  }, [fitView]);
  return null;
}

function GroupsCanvasInner({
  initialNodes,
  initialEdges,
  selectedGroupId,
  onConnectLink,
  onDisconnectLinks,
  onLayoutChange,
  onNodeContextMenu,
  onNodeClick,
  onDeleteGroup,
  onAddGroup,
}: GroupsCanvasProps) {
  const { getNodes } = useReactFlow();
  const [nodes, setNodes, onNodesChange] = useNodesState(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges);
  const [selectedEdgeIds, setSelectedEdgeIds] = useState<string[]>([]);
  const [linkMode, setLinkMode] = useState<LinkDrawMode>('bidirectional');

  const disconnectEdges = useCallback(async (toRemove: Edge[]) => {
    if (toRemove.length === 0) return;
    await onDisconnectLinks(toRemove);
    const removeIds = new Set(toRemove.map((e) => e.id));
    setEdges((eds) => eds.filter((e) => !removeIds.has(e.id)));
    setSelectedEdgeIds([]);
  }, [onDisconnectLinks, setEdges]);

  const onBeforeDelete = useCallback<OnBeforeDelete<Node<GroupNodeData>, Edge>>(async ({ nodes: nodesToDelete, edges: edgesToDelete }) => {
    if (nodesToDelete.length > 0) return false;
    if (edgesToDelete.length === 0) return false;
    try {
      await disconnectEdges(edgesToDelete);
    } catch (error) {
      alert(getErrorMessage(error, 'Failed to disconnect'));
    }
    return false;
  }, [disconnectEdges]);

  const onConnect = useCallback(async (connection: Connection) => {
    const { from, to } = connectionLinkEnds(connection);
    if (!from || !to || from === to) return;

    const bidirectional = linkMode === 'bidirectional';
    let shouldConnect = false;
    const currentNodes = getNodes() as Node<GroupNodeData>[];
    setEdges((eds) => {
      if (hasGroupLink(eds, from, to)) return eds;
      shouldConnect = true;
      return appendGroupEdge(eds, currentNodes, from, to, bidirectional);
    });
    if (!shouldConnect) return;

    try {
      await onConnectLink(from, to, bidirectional);
    } catch {
      setEdges((eds) => eds.filter((e) => e.id !== linkEdgeId(from, to, bidirectional)));
    }
  }, [linkMode, onConnectLink, setEdges, getNodes]);

  const isValidConnection = useCallback((connection: Connection | Edge) => {
    const from = Number(connection.source);
    const to = Number(connection.target);
    if (!from || !to || from === to) return false;
    return !hasGroupLink(edges, from, to);
  }, [edges]);

  const nodesWithDragged = useCallback(
    (draggedNode: Node<GroupNodeData>) => {
      const live = getNodes() as Node<GroupNodeData>[];
      return live.map((n) => (n.id === draggedNode.id ? draggedNode : n));
    },
    [getNodes],
  );

  const rematchDraggedNodeEdges = useCallback(
    (draggedNode: Node<GroupNodeData>) => {
      const allNodes = nodesWithDragged(draggedNode);
      setEdges((eds) => rematchEdgesForNode(eds, allNodes, draggedNode.id));
    },
    [nodesWithDragged, setEdges],
  );

  const onNodeDrag = useCallback(
    (_event: MouseEvent | TouchEvent, draggedNode: Node<GroupNodeData>) => {
      rematchDraggedNodeEdges(draggedNode);
    },
    [rematchDraggedNodeEdges],
  );

  const onNodeDragStop = useCallback(
    (_event: MouseEvent | TouchEvent, draggedNode: Node<GroupNodeData>) => {
      const allNodes = nodesWithDragged(draggedNode);
      setEdges((eds) => rematchEdgesForNode(eds, allNodes, draggedNode.id));
      void onLayoutChange(allNodes);
    },
    [nodesWithDragged, onLayoutChange, setEdges],
  );

  const onSelectionChange = useCallback(({ edges: selected }: { edges: Edge[] }) => {
    const ids = selected.map((e) => e.id);
    setSelectedEdgeIds((prev) => {
      if (prev.length === ids.length && prev.every((id, i) => id === ids[i])) return prev;
      return ids;
    });
  }, []);

  const onDisconnectSelected = useCallback(() => {
    void disconnectEdges(edges.filter((e) => selectedEdgeIds.includes(e.id)));
  }, [disconnectEdges, edges, selectedEdgeIds]);

  useEffect(() => {
    if (selectedGroupId == null) return;
    setNodes((nds) => nds.map((n) => ({
      ...n,
      selected: Number(n.id) === selectedGroupId,
    })));
  }, [selectedGroupId, setNodes]);

  return (
    <div style={{ width: '100%', height: '100%', minHeight: 0, flex: 1 }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        isValidConnection={isValidConnection}
        onBeforeDelete={onBeforeDelete}
        onNodeDrag={onNodeDrag}
        onNodeDragStop={onNodeDragStop}
        onNodeContextMenu={onNodeContextMenu}
        onNodeClick={onNodeClick}
        onSelectionChange={onSelectionChange}
        nodesDraggable
        nodesConnectable
        elementsSelectable
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        connectionMode={ConnectionMode.Loose}
        defaultEdgeOptions={defaultEdgeOptions}
        edgesReconnectable={false}
        deleteKeyCode={['Backspace', 'Delete']}
        proOptions={{ hideAttribution: true }}
        style={{ width: '100%', height: '100%' }}
      >
        <Background gap={20} size={1} />
        <FitViewOnMount />
        <Panel position="bottom-left">
          <LinkDrawToolbar mode={linkMode} onModeChange={setLinkMode} />
        </Panel>
        {(selectedEdgeIds.length > 0 || selectedGroupId != null) && (
          <Panel position="top-left">
            {selectedEdgeIds.length > 0 ? (
              <Button
                size="small"
                appearance="primary"
                icon={<DismissRegular />}
                onClick={onDisconnectSelected}
              >
                Disconnect
              </Button>
            ) : (
              <Button
                size="small"
                appearance="primary"
                icon={<DeleteRegular />}
                onClick={() => onDeleteGroup(selectedGroupId!)}
              >
                Delete group
              </Button>
            )}
          </Panel>
        )}
        <Panel position="top-right">
          <Button appearance="primary" size="small" icon={<AddRegular />} onClick={onAddGroup}>
            Add group
          </Button>
        </Panel>
      </ReactFlow>
    </div>
  );
}

export default function GroupsCanvas(props: GroupsCanvasProps) {
  return (
    <ReactFlowProvider>
      <GroupsCanvasInner key={props.revision} {...props} />
    </ReactFlowProvider>
  );
}
