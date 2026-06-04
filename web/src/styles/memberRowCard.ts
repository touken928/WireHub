import { makeStyles, tokens } from '@fluentui/react-components';

/** Row list card layout shared by Peers, Forward, and Maps. */
export const useMemberRowCardStyles = makeStyles({
  rowCard: {
    padding: '16px 18px',
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow2,
    display: 'grid',
    gap: '12px 16px',
    alignItems: 'center',
    '@media (max-width: 960px)': {
      gridTemplateColumns: '1fr',
    },
  },
  rowCardStats3: {
    gridTemplateColumns: 'minmax(160px, 1.2fr) repeat(3, minmax(0, 1fr)) auto',
  },
  rowCardStats4: {
    gridTemplateColumns: 'minmax(160px, 1.2fr) repeat(4, minmax(0, 1fr)) auto',
  },
  rowIdentity: {
    display: 'flex',
    flexDirection: 'column',
    gap: '6px',
    minWidth: 0,
  },
  nameRow: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    flexWrap: 'wrap',
  },
  subTag: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '4px',
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
    fontFamily: tokens.fontFamilyMonospace,
  },
  groupTag: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '4px',
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  rowStat: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
    minWidth: 0,
  },
  metaLabel: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  rowStatValue: {
    fontSize: tokens.fontSizeBase300,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  rowStatBody: {
    fontSize: tokens.fontSizeBase300,
    minWidth: 0,
  },
  mono: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase200,
  },
  rowActions: {
    display: 'flex',
    gap: '6px',
    flexWrap: 'wrap',
    justifyContent: 'flex-end',
    alignItems: 'center',
  },
});
