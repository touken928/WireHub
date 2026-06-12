import { addEdge, MarkerType, type Connection, type Edge, type Node } from '@xyflow/react';
import type { GroupGraph, GroupGraphNode } from '@/api/types';
import {
  applyEdgeHandles,
  autoLayoutGroups,
  pickEdgeHandles,
} from '@/components/groups/groupLayout';
import type { GroupLinkEdgeData, GroupNodeData } from '@/components/groups/types';

export const defaultEdgeOptions = {
  deletable: true,
  selectable: true,
  interactionWidth: 24,
};

export function linkEdgeId(from: number, to: number, bidirectional: boolean) {
  if (bidirectional) {
    const low = Math.min(from, to);
    const high = Math.max(from, to);
    return `${low}-${high}-bi`;
  }
  return `${from}-${to}-uni`;
}

export function edgeLinkEndpoints(edge: Edge): { from: number; to: number; bidirectional: boolean } {
  const data = edge.data as GroupLinkEdgeData | undefined;
  if (data?.fromGroupId != null && data?.toGroupId != null) {
    return {
      from: data.fromGroupId,
      to: data.toGroupId,
      bidirectional: data.bidirectional,
    };
  }
  return {
    from: Number(edge.source),
    to: Number(edge.target),
    bidirectional: true,
  };
}

/** True if any link already exists between the two groups (at most one edge per pair). */
export function hasGroupLink(edges: Edge[], a: number, b: number) {
  return edges.some((e) => {
    const { from, to } = edgeLinkEndpoints(e);
    return (from === a && to === b) || (from === b && to === a);
  });
}

function linkEdgeAppearance(
  bidirectional: boolean,
): Pick<Edge, 'type' | 'className' | 'markerStart' | 'markerEnd' | 'style' | 'animated'> {
  const arrow = { type: MarkerType.ArrowClosed, width: 14, height: 14 };
  if (bidirectional) {
    return {
      type: 'groupLink',
      markerStart: arrow,
      markerEnd: arrow,
      animated: false,
      style: { strokeWidth: 1.5 },
    };
  }
  return {
    type: 'groupLink',
    className: 'group-link-uni',
    markerEnd: { ...arrow, width: 16, height: 16 },
    animated: false,
    style: { strokeWidth: 1.5 },
  };
}

/**
 * Maps a new canvas connection to DB policy direction.
 * Must match repo GroupLink (from_group_id → to_group_id) and domain.LinkAllowsInit.
 *
 * In ConnectionMode.Loose with source+target handles on each node, React Flow's
 * connection.source/target can follow handle types instead of drag start/end.
 * For unidirectional links, pass connectStartNodeId from onConnectStart.
 */
export function connectionLinkEnds(
  connection: Connection,
  connectStartNodeId: string | null | undefined,
  bidirectional: boolean,
): { from: number; to: number } {
  const src = Number(connection.source);
  const tgt = Number(connection.target);
  if (!bidirectional && connectStartNodeId) {
    const from = Number(connectStartNodeId);
    const to = from === src ? tgt : src;
    return { from, to };
  }
  return { from: src, to: tgt };
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
    const bidirectional = l.bidirectional ?? true;
    const from = bidirectional ? Math.min(l.from_group_id, l.to_group_id) : l.from_group_id;
    const to = bidirectional ? Math.max(l.from_group_id, l.to_group_id) : l.to_group_id;
    return {
      id: linkEdgeId(l.from_group_id, l.to_group_id, bidirectional),
      source: String(from),
      target: String(to),
      data: {
        fromGroupId: l.from_group_id,
        toGroupId: l.to_group_id,
        bidirectional,
      } satisfies GroupLinkEdgeData,
      ...defaultEdgeOptions,
      ...linkEdgeAppearance(bidirectional),
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
  bidirectional: boolean,
) {
  if (hasGroupLink(edges, from, to)) return edges;
  const source = bidirectional ? Math.min(from, to) : from;
  const target = bidirectional ? Math.max(from, to) : to;
  const handles = pickEdgeHandles(String(source), String(target), nodes);
  const edge: Edge = {
    id: linkEdgeId(from, to, bidirectional),
    source: String(source),
    target: String(target),
    sourceHandle: handles.sourceHandle,
    targetHandle: handles.targetHandle,
    data: { fromGroupId: from, toGroupId: to, bidirectional } satisfies GroupLinkEdgeData,
    ...defaultEdgeOptions,
    ...linkEdgeAppearance(bidirectional),
  };
  return addEdge(edge, edges);
}
