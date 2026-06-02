import { BaseEdge, getSmoothStepPath, type EdgeProps } from '@xyflow/react';
import type { GroupLinkEdgeData } from '@/components/groups/types';

/** React Flow path + markerStart for uni arrow (markerEnd points backward on this path). */
export default function GroupLinkEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
  markerEnd,
  markerStart,
  style,
}: EdgeProps) {
  const link = data as GroupLinkEdgeData | undefined;
  const bidirectional = link?.bidirectional ?? true;

  const [path] = getSmoothStepPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    borderRadius: 14,
  });

  return (
    <BaseEdge
      id={id}
      path={path}
      style={style}
      markerStart={markerStart}
      markerEnd={bidirectional ? markerEnd : undefined}
    />
  );
}
