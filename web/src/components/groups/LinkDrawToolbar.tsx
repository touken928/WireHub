import {
  Switch,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import type { LinkDrawMode } from '@/components/groups/types';

const useStyles = makeStyles({
  panel: {
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
    padding: '8px 12px',
    borderRadius: tokens.borderRadiusMedium,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow4,
  },
  label: {
    fontSize: tokens.fontSizeBase300,
    color: tokens.colorNeutralForeground1,
    userSelect: 'none',
  },
});

type LinkDrawToolbarProps = {
  mode: LinkDrawMode;
  onModeChange: (mode: LinkDrawMode) => void;
};

export function LinkDrawToolbar({ mode, onModeChange }: LinkDrawToolbarProps) {
  const styles = useStyles();
  const bothWays = mode === 'bidirectional';

  return (
    <div className={styles.panel}>
      <Switch
        checked={bothWays}
        onChange={(_, data) => onModeChange(data.checked ? 'bidirectional' : 'unidirectional')}
        aria-label="Bidirectional link"
      />
      <Text className={styles.label}>{bothWays ? 'Both ways' : 'One-way'}</Text>
    </div>
  );
}
