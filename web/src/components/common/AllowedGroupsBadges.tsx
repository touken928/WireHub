import { Badge, Text, makeStyles, mergeClasses, tokens } from '@fluentui/react-components';
import { PeopleTeamRegular } from '@fluentui/react-icons';
import type { PeerGroup } from '@/api/types';
import { groupsForAllowedIds } from '@/lib/allowedGroups';

const useStyles = makeStyles({
  wrap: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: '6px',
    alignItems: 'center',
  },
  wrapCompact: {
    gap: '4px',
  },
  badge: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '4px',
  },
  empty: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
});

type AllowedGroupsBadgesProps = {
  groupIds: number[];
  groups: readonly PeerGroup[];
  compact?: boolean;
};

export function AllowedGroupsBadges({ groupIds, groups, compact }: AllowedGroupsBadgesProps) {
  const styles = useStyles();
  const resolved = groupsForAllowedIds(groupIds, groups);

  if (resolved.length === 0) {
    return <Text className={styles.empty}>—</Text>;
  }

  return (
    <div className={mergeClasses(styles.wrap, compact && styles.wrapCompact)}>
      {resolved.map((g) => (
        <Badge
          key={g.id}
          appearance="outline"
          color="informative"
          size={compact ? 'small' : 'medium'}
          className={styles.badge}
        >
          <PeopleTeamRegular fontSize={compact ? 12 : 14} />
          {g.name}
        </Badge>
      ))}
    </div>
  );
}
