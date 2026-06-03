import { makeStyles, tokens } from '@fluentui/react-components';
import { LAYOUT } from '@/styles/layout';

/** Shared shell for Dashboard, Groups, Peers, Settings. */
export const usePageLayoutStyles = makeStyles({
  page: {
    display: 'flex',
    flexDirection: 'column',
    gap: LAYOUT.pageGap,
    width: '100%',
  },
  /** Groups: fill remaining viewport height below the app chrome. */
  pageFill: {
    flex: 1,
    minHeight: 0,
    overflow: 'hidden',
  },
  muted: {
    color: tokens.colorNeutralForeground3,
  },
});
