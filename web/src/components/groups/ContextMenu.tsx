import { makeStyles, tokens } from '@fluentui/react-components';
import type { ReactNode } from 'react';

const useStyles = makeStyles({
  root: {
    position: 'fixed',
    zIndex: 1000,
    backgroundColor: tokens.colorNeutralBackground1,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    borderRadius: tokens.borderRadiusMedium,
    boxShadow: tokens.shadow8,
    padding: '4px',
    display: 'flex',
    flexDirection: 'column',
    minWidth: '160px',
  },
});

type ContextMenuProps = {
  x: number;
  y: number;
  children: ReactNode;
};

export function ContextMenu({ x, y, children }: ContextMenuProps) {
  const styles = useStyles();
  return (
    <div
      className={styles.root}
      style={{ left: x, top: y }}
      onClick={(e) => e.stopPropagation()}
    >
      {children}
    </div>
  );
}
