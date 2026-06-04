import {
  Button,
  Checkbox,
  Field,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { PeopleTeamRegular } from '@fluentui/react-icons';
import { useMemo } from 'react';
import type { PeerGroup } from '@/api/types';
import {
  allowedGroupsSummary,
  clearAllowedGroups,
  selectAllGroupIds,
  toggleAllowedGroup,
} from '@/lib/allowedGroups';

const useStyles = makeStyles({
  field: {
    display: 'flex',
    flexDirection: 'column',
    gap: '8px',
  },
  header: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    gap: '12px',
    flexWrap: 'wrap',
  },
  hint: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    maxWidth: '420px',
  },
  toolbar: {
    display: 'flex',
    gap: '4px',
    flexShrink: 0,
  },
  panel: {
    borderRadius: tokens.borderRadiusMedium,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    overflow: 'hidden',
  },
  panelEmpty: {
    padding: '16px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase300,
  },
  grid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))',
    gap: '1px',
    backgroundColor: tokens.colorNeutralStroke2,
  },
  item: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    padding: '10px 12px',
    backgroundColor: tokens.colorNeutralBackground1,
    cursor: 'pointer',
    minHeight: '44px',
    ':hover': {
      backgroundColor: tokens.colorNeutralBackground1Hover,
    },
  },
  itemSelected: {
    backgroundColor: tokens.colorBrandBackground2,
    ':hover': {
      backgroundColor: tokens.colorBrandBackground2Hover,
    },
  },
  itemLabel: {
    flex: 1,
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    fontSize: tokens.fontSizeBase300,
  },
  footer: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '8px',
    padding: '8px 12px',
    borderTop: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground3,
  },
  summary: {
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
  },
  summaryWarn: {
    color: tokens.colorPaletteDarkOrangeForeground1,
  },
  iconMuted: {
    color: tokens.colorNeutralForeground3,
    flexShrink: 0,
  },
  iconSelected: {
    color: tokens.colorBrandForeground1,
    flexShrink: 0,
  },
});

type AllowedGroupsPickerProps = {
  groups: readonly PeerGroup[];
  value: number[];
  onChange: (ids: number[]) => void;
  disabled?: boolean;
};

export function AllowedGroupsPicker({ groups, value, onChange, disabled }: AllowedGroupsPickerProps) {
  const styles = useStyles();
  const sorted = useMemo(
    () => [...groups].sort((a, b) => a.name.localeCompare(b.name)),
    [groups],
  );
  const selectedSet = useMemo(() => new Set(value), [value]);
  const summary = allowedGroupsSummary(value.length, sorted.length);
  const noneSelected = value.length === 0 && sorted.length > 0;

  const setChecked = (groupId: number, checked: boolean) => {
    onChange(toggleAllowedGroup(value, groupId, checked));
  };

  return (
    <Field
      className={styles.field}
      label="Allowed groups"
      required
      hint={
        <span className={styles.hint}>
          Default deny: only peers in selected groups can resolve DNS and reach this map.
        </span>
      }
    >
      <div className={styles.header}>
        <div />
        {sorted.length > 0 && (
          <div className={styles.toolbar}>
            <Button
              size="small"
              appearance="subtle"
              disabled={disabled || value.length === sorted.length}
              onClick={() => onChange(selectAllGroupIds(sorted))}
            >
              Select all
            </Button>
            <Button
              size="small"
              appearance="subtle"
              disabled={disabled || value.length === 0}
              onClick={() => onChange(clearAllowedGroups())}
            >
              Clear
            </Button>
          </div>
        )}
      </div>

      <div className={styles.panel}>
        {sorted.length === 0 ? (
          <div className={styles.panelEmpty}>
            Create a group under <strong>Groups</strong> before configuring map access.
          </div>
        ) : (
          <div className={styles.grid} role="group" aria-label="Allowed groups">
            {sorted.map((g) => {
              const checked = selectedSet.has(g.id);
              return (
                <label
                  key={g.id}
                  className={`${styles.item} ${checked ? styles.itemSelected : ''}`}
                >
                  <Checkbox
                    checked={checked}
                    disabled={disabled}
                    onChange={(_, d) => setChecked(g.id, Boolean(d.checked))}
                    aria-label={g.name}
                  />
                  <PeopleTeamRegular
                    className={checked ? styles.iconSelected : styles.iconMuted}
                    fontSize={18}
                  />
                  <span className={styles.itemLabel}>{g.name}</span>
                </label>
              );
            })}
          </div>
        )}
        {sorted.length > 0 && (
          <div className={styles.footer}>
            <Text className={`${styles.summary} ${noneSelected ? styles.summaryWarn : ''}`}>
              {summary}
            </Text>
          </div>
        )}
      </div>
    </Field>
  );
}
