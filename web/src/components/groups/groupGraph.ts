import { addEdge, type Connection, type Edge, type Node } from '@xyflow/react';
import type { GroupGraph, GroupGraphNode } from '@/api/types';
import {
  applyEdgeHandles,
  autoLayoutGroups,
  pickEdgeHandles,
} from '@/components/groups/groupLayout';
import type { GroupNodeData } from '@/components/groups/types';

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
  graph: Pick<GroupGraph, 'groups' | 'links'> & {
    groups?: GroupGraphNode[];
    links?: GroupGraph['links'];
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
