import { Subtitle2, Text, makeStyles } from '@fluentui/react-components';
import type { ReactNode } from 'react';
import { LAYOUT } from '@/styles/layout';
import { usePageLayoutStyles } from '@/styles/pageLayout';

const useStyles = makeStyles({
  root: {
    flexShrink: 0,
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: '12px 16px',
  },
  main: {
    flex: '1 1 240px',
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: LAYOUT.pageHeaderGap,
  },
  actions: {
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    paddingTop: '2px',
  },
});

type PageHeaderProps = {
  title: string;
  description?: string;
  actions?: ReactNode;
};

export function PageHeader({ title, description, actions }: PageHeaderProps) {
  const styles = useStyles();
  const page = usePageLayoutStyles();

  return (
    <header className={styles.root}>
      <div className={styles.main}>
        <Subtitle2>{title}</Subtitle2>
        {description ? (
          <Text size={200} className={page.muted}>
            {description}
          </Text>
        ) : null}
      </div>
      {actions ? <div className={styles.actions}>{actions}</div> : null}
    </header>
  );
}
