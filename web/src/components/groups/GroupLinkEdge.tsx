import { BaseEdge, getSmoothStepPath, type EdgeProps } from '@xyflow/react';
import type { GroupLinkEdgeData } from '@/components/groups/types';

/** Uni: markerEnd at path target (same direction as dash flow and from_group_id → to_group_id). */
export default function GroupLinkEdge({
  id,
  source,
  target,
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

  const policyTowardTarget =
    bidirectional
    || (link?.fromGroupId != null
      && source === String(link.fromGroupId)
      && target === String(link.toGroupId));

  const uniStyle = bidirectional
    ? style
    : {
        ...style,
        strokeDasharray: '6 4',
        animation: policyTowardTarget
          ? `group-link-uni-to-target 0.55s linear infinite`
          : `group-link-uni-to-target-rev 0.55s linear infinite`,
      };

  return (
    <BaseEdge
      id={id}
      path={path}
      style={uniStyle}
      markerStart={bidirectional ? markerStart : undefined}
      markerEnd={markerEnd}
    />
  );
}
