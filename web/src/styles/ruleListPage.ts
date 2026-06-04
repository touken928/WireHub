import { makeStyles, tokens } from '@fluentui/react-components';

/** List page chrome for Forward / Maps. */
export const useRuleListPageStyles = makeStyles({
  toolbar: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: '10px',
    alignItems: 'flex-end',
  },
  searchField: {
    flex: '1 1 220px',
    minWidth: '200px',
  },
  resultHint: {
    color: tokens.colorNeutralForeground3,
    fontSize: tokens.fontSizeBase200,
  },
  list: {
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  empty: {
    padding: '32px',
    textAlign: 'center',
    color: tokens.colorNeutralForeground3,
    borderRadius: tokens.borderRadiusXLarge,
    border: `1px dashed ${tokens.colorNeutralStroke2}`,
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
    fontSize: tokens.fontSizeBase300,
  },
});
