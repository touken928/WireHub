import { Position, type Edge, type Node } from '@xyflow/react';
import type { GroupGraphNode } from '@/api/types';
import type { GroupLinkEdgeData, GroupNodeData } from '@/components/groups/types';

export const GROUP_NODE_W = 140;
export const GROUP_NODE_H = 70;

export function handleId(side: Position, role: 'source' | 'target') {
  return `${side}-${role}`;
}

export function autoLayoutGroups(groups: GroupGraphNode[]): Map<number, { x: number; y: number }> {
  const positions = new Map<number, { x: number; y: number }>();
  const n = groups.length;
  if (n === 0) return positions;
  if (n === 1) {
    positions.set(groups[0].id, { x: 0, y: 0 });
    return positions;
  }
  const radius = Math.max(220, n * 55);
  groups.forEach((g, i) => {
    const angle = (2 * Math.PI * i) / n - Math.PI / 2;
    positions.set(g.id, {
      x: radius * Math.cos(angle) - GROUP_NODE_W / 2,
      y: radius * Math.sin(angle) - GROUP_NODE_H / 2,
    });
  });
  return positions;
}

const SIDES = [Position.Top, Position.Right, Position.Bottom, Position.Left] as const;

function nodeCenter(node: Node<GroupNodeData>) {
  return {
    x: node.position.x + GROUP_NODE_W / 2,
    y: node.position.y + GROUP_NODE_H / 2,
  };
}

function sideAnchor(node: Node<GroupNodeData>, side: Position) {
  const c = nodeCenter(node);
  switch (side) {
    case Position.Top:
      return { x: c.x, y: node.position.y };
    case Position.Bottom:
      return { x: c.x, y: node.position.y + GROUP_NODE_H };
    case Position.Left:
      return { x: node.position.x, y: c.y };
    case Position.Right:
      return { x: node.position.x + GROUP_NODE_W, y: c.y };
    default:
      return c;
  }
}

/** Pick source/target handles on the sides closest to the peer node. */
function nearestSide(node: Node<GroupNodeData>, toward: { x: number; y: number }): Position {
  let best: Position = Position.Top;
  let bestDist = Infinity;
  for (const side of SIDES) {
    const anchor = sideAnchor(node, side);
    const dist = (anchor.x - toward.x) ** 2 + (anchor.y - toward.y) ** 2;
    if (dist < bestDist) {
      bestDist = dist;
      best = side;
    }
  }
  return best;
}

export function pickEdgeHandles(
  sourceId: string,
  targetId: string,
  nodes: Node<GroupNodeData>[],
): { sourceHandle: string; targetHandle: string } {
  const source = nodes.find((n) => n.id === sourceId);
  const target = nodes.find((n) => n.id === targetId);
  if (!source || !target) {
    return {
      sourceHandle: handleId(Position.Bottom, 'source'),
      targetHandle: handleId(Position.Top, 'source'),
    };
  }

  const tCenter = nodeCenter(target);
  const sCenter = nodeCenter(source);
  const srcSide = nearestSide(source, tCenter);
  const tgtSide = nearestSide(target, sCenter);
  return {
    sourceHandle: handleId(srcSide, 'source'),
    targetHandle: handleId(tgtSide, 'source'),
  };
}

function directedEndpoints(edge: Edge): { sourceId: string; targetId: string } {
  const data = edge.data as GroupLinkEdgeData | undefined;
  if (data?.fromGroupId != null && data?.toGroupId != null && !data.bidirectional) {
    return { sourceId: String(data.fromGroupId), targetId: String(data.toGroupId) };
  }
  return { sourceId: edge.source, targetId: edge.target };
}

function withNearestHandles(
  edge: Edge,
  nodes: Node<GroupNodeData>[],
): Edge {
  const { sourceId, targetId } = directedEndpoints(edge);
  const handles = pickEdgeHandles(sourceId, targetId, nodes);
  const data = edge.data as GroupLinkEdgeData | undefined;
  const uni = data && !data.bidirectional;
  return {
    ...edge,
    ...(uni ? { source: sourceId, target: targetId } : {}),
    sourceHandle: handles.sourceHandle,
    targetHandle: handles.targetHandle,
  };
}

export function applyEdgeHandles(edges: Edge[], nodes: Node<GroupNodeData>[]): Edge[] {
  return edges.map((edge) => withNearestHandles(edge, nodes));
}

/** Recompute handles for every edge incident to nodeId (after drag). */
export function rematchEdgesForNode(
  edges: Edge[],
  nodes: Node<GroupNodeData>[],
  nodeId: string,
): Edge[] {
  return applyEdgeHandlesForNode(edges, nodes, nodeId);
}

export function applyEdgeHandlesForNode(
  edges: Edge[],
  nodes: Node<GroupNodeData>[],
  nodeId: string,
): Edge[] {
  return edges.map((edge) => {
    const { sourceId, targetId } = directedEndpoints(edge);
    if (sourceId !== nodeId && targetId !== nodeId) return edge;
    return withNearestHandles(edge, nodes);
  });
}
