import {
  useCallback,
  useEffect,
  useState,
} from 'react';
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  addEdge,
  useNodesState,
  useEdgesState,
  useReactFlow,
  type Connection,
  type Edge,
  type Node,
  type NodeTypes,
  type OnBeforeDelete,
  ConnectionMode,
  Panel,
} from '@xyflow/react';
import { Button } from '@fluentui/react-components';
import { AddRegular, DeleteRegular, DismissRegular } from '@fluentui/react-icons';
import type { GroupGraphNode } from '../api/client';
import GroupNode from './GroupNode';
import type { GroupNodeData } from './groupNodeTypes';
import {
  applyEdgeHandles,
  applyEdgeHandlesForNode,
  autoLayoutGroups,
  pickEdgeHandles,
} from './groupLayout';

const nodeTypes: NodeTypes = {
  peerGroup: GroupNode,
};

const defaultEdgeOptions = {
  deletable: true,
  selectable: true,
  interactionWidth: 24,
};

export function linkEdgeId(a: number, b: number) {
  return `${Math.min(a, b)}-${Math.max(a, b)}`;
}

export function hasGroupLink(edges: Edge[], a: number, b: number) {
  return edges.some((e) => e.id === linkEdgeId(a, b));
}

export function graphToFlow(
  graph: {
    groups?: GroupGraphNode[];
    links?: { from_group_id: number; to_group_id: number }[];
  },
  options?: { autoLayout?: boolean },
) {
  const groups = graph.groups ?? [];
  const links = graph.links ?? [];
  const layout = options?.autoLayout ? autoLayoutGroups(groups) : null;
  const nodes: Node<GroupNodeData>[] = groups.map((g) => ({
    id: String(g.id),
    type: 'peerGroup',
    position: layout?.get(g.id) ?? { x: g.pos_x || 0, y: g.pos_y || 0 },
    data: {
      label: g.name,
      groupId: g.id,
    },
    deletable: false,
  }));
  const rawEdges: Edge[] = links.map((l) => {
    const low = Math.min(l.from_group_id, l.to_group_id);
    const high = Math.max(l.from_group_id, l.to_group_id);
    return {
      id: `${low}-${high}`,
      source: String(low),
      target: String(high),
      animated: true,
      ...defaultEdgeOptions,
    };
  });
  const edges = applyEdgeHandles(rawEdges, nodes);
  const layoutPayload = nodes.map((n) => ({
    id: Number(n.id),
    pos_x: n.position.x,
    pos_y: n.position.y,
  }));
  return { nodes, edges, layoutPayload };
}

export function appendGroupEdge(
  edges: Edge[],
  nodes: Node<GroupNodeData>[],
  from: number,
  to: number,
  connection?: Pick<Connection, 'sourceHandle' | 'targetHandle'>,
) {
  if (hasGroupLink(edges, from, to)) return edges;
  const low = Math.min(from, to);
  const high = Math.max(from, to);
  const handles = connection?.sourceHandle && connection?.targetHandle
    ? { sourceHandle: connection.sourceHandle, targetHandle: connection.targetHandle }
    : pickEdgeHandles(String(low), String(high), nodes);
  return addEdge({
    id: linkEdgeId(from, to),
    source: String(low),
    target: String(high),
    sourceHandle: handles.sourceHandle,
    targetHandle: handles.targetHandle,
    animated: true,
    ...defaultEdgeOptions,
  }, edges);
}

type GroupsCanvasProps = {
  revision: number;
  initialNodes: Node<GroupNodeData>[];
  initialEdges: Edge[];
  selectedGroupId: number | null;
  onConnectLink: (from: number, to: number) => Promise<void>;
  onDisconnectLinks: (edges: Edge[]) => Promise<void>;
  onLayoutChange: (nodes: Node<GroupNodeData>[]) => Promise<void>;
  onEdgeContextMenu: (event: React.MouseEvent, edge: Edge) => void;
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
  onEdgeContextMenu,
  onNodeContextMenu,
  onNodeClick,
  onDeleteGroup,
  onAddGroup,
}: GroupsCanvasProps) {
  const { getNodes } = useReactFlow();
  const [nodes, setNodes, onNodesChange] = useNodesState(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges);
  const [selectedEdgeIds, setSelectedEdgeIds] = useState<string[]>([]);

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
    } catch (e) {
      alert(e instanceof Error ? e.message : 'Failed to disconnect');
    }
    return false;
  }, [disconnectEdges]);

  const onConnect = useCallback(async (connection: Connection) => {
    const from = Number(connection.source);
    const to = Number(connection.target);
    if (!from || !to || from === to) return;

    let shouldConnect = false;
    const currentNodes = getNodes() as Node<GroupNodeData>[];
    setEdges((eds) => {
      if (hasGroupLink(eds, from, to)) return eds;
      shouldConnect = true;
      return appendGroupEdge(eds, currentNodes, from, to, connection);
    });
    if (!shouldConnect) return;

    try {
      await onConnectLink(from, to);
    } catch {
      setEdges((eds) => eds.filter((e) => e.id !== linkEdgeId(from, to)));
    }
  }, [onConnectLink, setEdges, getNodes]);

  const isValidConnection = useCallback((connection: Connection | Edge) => {
    const from = Number(connection.source);
    const to = Number(connection.target);
    if (!from || !to || from === to) return false;
    return !hasGroupLink(edges, from, to);
  }, [edges]);

  const onNodeDragStop = useCallback((_event: MouseEvent | TouchEvent, draggedNode: Node<GroupNodeData>) => {
    const allNodes = getNodes() as Node<GroupNodeData>[];
    setEdges((eds) => applyEdgeHandlesForNode(eds, allNodes, draggedNode.id));
    void onLayoutChange(allNodes);
  }, [getNodes, onLayoutChange, setEdges]);

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
        onNodeDragStop={onNodeDragStop}
        onEdgeContextMenu={onEdgeContextMenu}
        onNodeContextMenu={onNodeContextMenu}
        onNodeClick={onNodeClick}
        onSelectionChange={onSelectionChange}
        nodesDraggable
        nodesConnectable
        elementsSelectable
        nodeTypes={nodeTypes}
        connectionMode={ConnectionMode.Loose}
        defaultEdgeOptions={defaultEdgeOptions}
        edgesReconnectable={false}
        deleteKeyCode={['Backspace', 'Delete']}
        proOptions={{ hideAttribution: true }}
        style={{ width: '100%', height: '100%' }}
      >
        <Background gap={20} size={1} />
        <FitViewOnMount />
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
      <GroupsCanvasInner
        key={props.revision}
        {...props}
      />
    </ReactFlowProvider>
  );
}
