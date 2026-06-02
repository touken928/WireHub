import { Subtitle2, Text, makeStyles } from '@fluentui/react-components';
import { LAYOUT } from '../styles/layout';
import { usePageLayoutStyles } from '../styles/pageLayout';

const useStyles = makeStyles({
  root: {
    flexShrink: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: LAYOUT.pageHeaderGap,
  },
});

interface PageHeaderProps {
  title: string;
  description?: string;
}

export default function PageHeader({ title, description }: PageHeaderProps) {
  const styles = useStyles();
  const page = usePageLayoutStyles();

  return (
    <header className={styles.root}>
      <Subtitle2>{title}</Subtitle2>
      {description ? (
        <Text size={200} className={page.muted}>
          {description}
        </Text>
      ) : null}
    </header>
  );
}
