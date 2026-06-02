import { makeStyles, tokens } from '@fluentui/react-components';
import { LAYOUT } from '@/styles/layout';

export const useAuthLayoutStyles = makeStyles({
  page: {
    minHeight: '100vh',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '24px',
    background: `linear-gradient(135deg, ${tokens.colorBrandBackground2} 0%, ${tokens.colorNeutralBackground2} 100%)`,
  },
  card: {
    width: `min(${LAYOUT.authPanelWidth}, calc(100vw - 48px))`,
    padding: '32px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
  },
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  divider: {
    borderTop: `1px solid ${tokens.colorNeutralStroke2}`,
    margin: '8px 0',
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
  },
  success: {
    color: tokens.colorPaletteGreenForeground1,
  },
});
