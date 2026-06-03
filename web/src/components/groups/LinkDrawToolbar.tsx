import { makeStyles, tokens } from '@fluentui/react-components';
import type { LinkDrawMode } from '@/components/groups/types';

const useStyles = makeStyles({
  panel: {
    display: 'flex',
    gap: '6px',
    padding: '6px',
    borderRadius: tokens.borderRadiusMedium,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow4,
  },
  option: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '52px',
    height: '36px',
    padding: 0,
    borderRadius: tokens.borderRadiusSmall,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    cursor: 'pointer',
    color: tokens.colorNeutralForeground2,
    ':hover': {
      backgroundColor: tokens.colorNeutralBackground1Hover,
      color: tokens.colorNeutralForeground1,
    },
  },
  optionActive: {
    borderTopColor: tokens.colorBrandStroke1,
    borderRightColor: tokens.colorBrandStroke1,
    borderBottomColor: tokens.colorBrandStroke1,
    borderLeftColor: tokens.colorBrandStroke1,
    backgroundColor: tokens.colorBrandBackground2,
    color: tokens.colorBrandForeground1,
    ':hover': {
      backgroundColor: tokens.colorBrandBackground2Hover,
      color: tokens.colorBrandForeground1,
    },
  },
  icon: {
    display: 'block',
  },
});

type LinkModeIconProps = {
  mode: 'bidirectional' | 'unidirectional';
  className?: string;
};

function LinkModeIcon({ mode, className }: LinkModeIconProps) {
  if (mode === 'bidirectional') {
    return (
      <svg className={className} viewBox="0 0 44 16" width="44" height="16" aria-hidden>
        <line x1="12" y1="8" x2="32" y2="8" stroke="currentColor" strokeWidth="1.5" />
        <path d="M8 8 L12 5 V11 Z" fill="currentColor" />
        <path d="M36 8 L32 5 V11 Z" fill="currentColor" />
      </svg>
    );
  }
  return (
    <svg className={className} viewBox="0 0 44 16" width="44" height="16" aria-hidden>
      <line
        x1="8"
        y1="8"
        x2="32"
        y2="8"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeDasharray="4 3"
      />
      <path d="M36 8 L32 5 V11 Z" fill="currentColor" />
    </svg>
  );
}

type LinkDrawToolbarProps = {
  mode: LinkDrawMode;
  onModeChange: (mode: LinkDrawMode) => void;
};

export function LinkDrawToolbar({ mode, onModeChange }: LinkDrawToolbarProps) {
  const styles = useStyles();

  return (
    <div className={styles.panel} role="group" aria-label="Link direction">
      <button
        type="button"
        className={`${styles.option} ${mode === 'bidirectional' ? styles.optionActive : ''}`}
        aria-label="Bidirectional link"
        aria-pressed={mode === 'bidirectional'}
        onClick={() => onModeChange('bidirectional')}
      >
        <LinkModeIcon mode="bidirectional" className={styles.icon} />
      </button>
      <button
        type="button"
        className={`${styles.option} ${mode === 'unidirectional' ? styles.optionActive : ''}`}
        aria-label="Unidirectional link"
        aria-pressed={mode === 'unidirectional'}
        onClick={() => onModeChange('unidirectional')}
      >
        <LinkModeIcon mode="unidirectional" className={styles.icon} />
      </button>
    </div>
  );
}
