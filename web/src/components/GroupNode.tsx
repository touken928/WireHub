import { memo, Fragment } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { Text, makeStyles, tokens } from '@fluentui/react-components';
import type { GroupNodeData } from './groupNodeTypes';
import { handleId } from './groupLayout';

const SIDES = [
  Position.Top,
  Position.Right,
  Position.Bottom,
  Position.Left,
] as const;

const useStyles = makeStyles({
  root: {
    padding: '8px 12px',
    minWidth: '120px',
    textAlign: 'center',
    cursor: 'grab',
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground1,
  },
  selected: {
    border: `2px solid ${tokens.colorBrandStroke1}`,
    boxShadow: tokens.shadow4,
  },
});

function GroupNode({ data, selected }: NodeProps & { data: GroupNodeData }) {
  const styles = useStyles();
  const label = data?.label ?? 'Group';

  return (
    <div className={`${styles.root} ${selected ? styles.selected : ''}`}>
      {SIDES.map((pos) => (
        <Fragment key={pos}>
          <Handle id={handleId(pos, 'source')} type="source" position={pos} />
          <Handle id={handleId(pos, 'target')} type="target" position={pos} />
        </Fragment>
      ))}
      <Text weight="semibold">{label}</Text>
    </div>
  );
}

export default memo(GroupNode);
