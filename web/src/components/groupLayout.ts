import { Position, type Edge, type Node } from '@xyflow/react';
import type { GroupGraphNode } from '../api/client';
import type { GroupNodeData } from './groupNodeTypes';

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

function nodeCenter(node: Node<GroupNodeData>) {
  return {
    x: node.position.x + GROUP_NODE_W / 2,
    y: node.position.y + GROUP_NODE_H / 2,
  };
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
      targetHandle: handleId(Position.Top, 'target'),
    };
  }

  const s = nodeCenter(source);
  const t = nodeCenter(target);
  const dx = t.x - s.x;
  const dy = t.y - s.y;

  if (Math.abs(dx) > Math.abs(dy)) {
    return dx > 0
      ? {
          sourceHandle: handleId(Position.Right, 'source'),
          targetHandle: handleId(Position.Left, 'target'),
        }
      : {
          sourceHandle: handleId(Position.Left, 'source'),
          targetHandle: handleId(Position.Right, 'target'),
        };
  }
  return dy > 0
    ? {
        sourceHandle: handleId(Position.Bottom, 'source'),
        targetHandle: handleId(Position.Top, 'target'),
      }
    : {
        sourceHandle: handleId(Position.Top, 'source'),
        targetHandle: handleId(Position.Bottom, 'target'),
      };
}

export function applyEdgeHandles(edges: Edge[], nodes: Node<GroupNodeData>[]): Edge[] {
  return edges.map((edge) => {
    const handles = pickEdgeHandles(edge.source, edge.target, nodes);
    return {
      ...edge,
      sourceHandle: handles.sourceHandle,
      targetHandle: handles.targetHandle,
    };
  });
}

/** Recompute handles only for edges incident to a moved node. */
export function applyEdgeHandlesForNode(
  edges: Edge[],
  nodes: Node<GroupNodeData>[],
  nodeId: string,
): Edge[] {
  return edges.map((edge) => {
    if (edge.source !== nodeId && edge.target !== nodeId) return edge;
    const handles = pickEdgeHandles(edge.source, edge.target, nodes);
    return {
      ...edge,
      sourceHandle: handles.sourceHandle,
      targetHandle: handles.targetHandle,
    };
  });
}
