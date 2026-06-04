import { Card, Text, mergeClasses } from '@fluentui/react-components';
import type { ReactNode } from 'react';
import { useMemberRowCardStyles } from '@/styles/memberRowCard';

type MemberRowCardProps = {
  statColumns: 3 | 4;
  children: ReactNode;
};

export function MemberRowCard({ statColumns, children }: MemberRowCardProps) {
  const styles = useMemberRowCardStyles();
  return (
    <Card
      className={mergeClasses(
        styles.rowCard,
        statColumns === 3 ? styles.rowCardStats3 : styles.rowCardStats4,
      )}
    >
      {children}
    </Card>
  );
}

type MemberRowIdentityProps = {
  title: string;
  badge?: ReactNode;
  subtitle?: string;
  children?: ReactNode;
};

export function MemberRowIdentity({ title, badge, subtitle, children }: MemberRowIdentityProps) {
  const styles = useMemberRowCardStyles();
  return (
    <div className={styles.rowIdentity}>
      <div className={styles.nameRow}>
        <Text weight="semibold">{title}</Text>
        {badge}
      </div>
      {subtitle ? <span className={styles.subTag}>{subtitle}</span> : null}
      {children}
    </div>
  );
}

type MemberRowStatProps = {
  label: string;
  value: ReactNode;
  mono?: boolean;
};

export function MemberRowStat({ label, value, mono }: MemberRowStatProps) {
  const styles = useMemberRowCardStyles();
  const valueClass = mergeClasses(styles.rowStatValue, mono && styles.mono);
  return (
    <div className={styles.rowStat}>
      <span className={styles.metaLabel}>{label}</span>
      {typeof value === 'string' ? (
        <span className={valueClass} title={value}>{value}</span>
      ) : (
        <div className={styles.rowStatBody}>{value}</div>
      )}
    </div>
  );
}

type MemberRowActionsProps = {
  children: ReactNode;
};

export function MemberRowActions({ children }: MemberRowActionsProps) {
  const styles = useMemberRowCardStyles();
  return <div className={styles.rowActions}>{children}</div>;
}
