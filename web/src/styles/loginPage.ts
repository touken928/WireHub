import { makeStyles } from '@fluentui/react-components';
import type { CSSProperties } from 'react';

const MD = '@media (min-width: 768px)';

export const LOGIN_COLORS = {
  heroBg: '#000000',
  panelBg: '#f5f5f7',
  primaryText: '#1d1d1f',
  secondaryText: 'rgba(0, 0, 0, 0.48)',
  heroMuted: 'rgba(255, 255, 255, 0.6)',
  accent: '#0071e3',
  accentHover: '#0077ed',
  error: '#ff3b30',
  errorBg: 'rgba(255, 59, 48, 0.08)',
  inputBorder: 'rgba(0, 0, 0, 0.12)',
  inputShadow: '0 1px 2px rgba(0, 0, 0, 0.04)',
} as const;

export const useLoginPageStyles = makeStyles({
  shell: {
    minHeight: '100dvh',
    display: 'flex',
    flexDirection: 'column',
    backgroundColor: LOGIN_COLORS.panelBg,
    color: LOGIN_COLORS.primaryText,
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
    [MD]: {
      flexDirection: 'row',
    },
  },
  hero: {
    display: 'none',
    [MD]: {
      display: 'flex',
      flex: '1 1 50%',
      flexDirection: 'column',
      justifyContent: 'space-between',
      padding: '48px 56px',
      backgroundColor: LOGIN_COLORS.heroBg,
      color: '#ffffff',
    },
  },
  heroInner: {
    display: 'flex',
    flexDirection: 'column',
    gap: '28px',
    maxWidth: '520px',
  },
  heroFooter: {
    color: LOGIN_COLORS.heroMuted,
    fontSize: '14px',
    letterSpacing: '-0.28px',
    lineHeight: 1.4,
  },
  formPanel: {
    flex: '1 1 auto',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '32px 24px 48px',
    [MD]: {
      flex: '1 1 50%',
      padding: '48px 32px',
    },
  },
  mobileBrand: {
    display: 'flex',
    alignItems: 'center',
    gap: '12px',
    marginBottom: '32px',
    alignSelf: 'flex-start',
    width: '100%',
    maxWidth: '360px',
    [MD]: {
      display: 'none',
    },
  },
  mobileBrandName: {
    fontSize: '20px',
    fontWeight: 600,
    letterSpacing: '-0.374px',
    color: LOGIN_COLORS.primaryText,
  },
  formCard: {
    width: '100%',
    maxWidth: '360px',
    display: 'flex',
    flexDirection: 'column',
    gap: '24px',
  },
  formTitle: {
    margin: 0,
    fontSize: '28px',
    fontWeight: 600,
    letterSpacing: '-0.374px',
    lineHeight: 1.15,
    color: LOGIN_COLORS.primaryText,
  },
  formSubtitle: {
    margin: 0,
    fontSize: '14px',
    letterSpacing: '-0.28px',
    lineHeight: 1.45,
    color: LOGIN_COLORS.secondaryText,
  },
  form: {
    display: 'flex',
    flexDirection: 'column',
    gap: '20px',
  },
  field: {
    display: 'flex',
    flexDirection: 'column',
    gap: '8px',
  },
  fieldLabel: {
    fontSize: '12px',
    fontWeight: 500,
    letterSpacing: '-0.12px',
    color: LOGIN_COLORS.secondaryText,
  },
  inputRoot: {
    width: '100%',
    minWidth: 0,
    backgroundColor: '#ffffff',
    border: `1px solid ${LOGIN_COLORS.inputBorder}`,
    borderRadius: '12px',
    boxShadow: LOGIN_COLORS.inputShadow,
    '::after': {
      display: 'none',
    },
  },
  inputField: {
    height: '44px',
    paddingLeft: '14px',
    paddingRight: '14px',
    fontSize: '17px',
    letterSpacing: '-0.28px',
    color: LOGIN_COLORS.primaryText,
    '::placeholder': {
      color: LOGIN_COLORS.secondaryText,
    },
  },
  errorBanner: {
    padding: '12px 14px',
    borderRadius: '12px',
    backgroundColor: LOGIN_COLORS.errorBg,
    color: LOGIN_COLORS.error,
    fontSize: '14px',
    letterSpacing: '-0.28px',
    lineHeight: 1.4,
  },
  submitButton: {
    width: '100%',
    minHeight: '44px',
    borderRadius: '12px',
    backgroundColor: LOGIN_COLORS.accent,
    color: '#ffffff',
    fontSize: '17px',
    fontWeight: 600,
    letterSpacing: '-0.28px',
    border: 'none',
    ':hover': {
      backgroundColor: LOGIN_COLORS.accentHover,
      color: '#ffffff',
    },
    ':hover:active': {
      backgroundColor: LOGIN_COLORS.accentHover,
      color: '#ffffff',
    },
    ':disabled': {
      opacity: 0.5,
      backgroundColor: LOGIN_COLORS.accent,
      color: '#ffffff',
    },
    ':disabled:hover': {
      backgroundColor: LOGIN_COLORS.accent,
      color: '#ffffff',
    },
  },
  logoMark: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '44px',
    height: '44px',
    borderRadius: '12px',
    backgroundColor: LOGIN_COLORS.accent,
    color: '#ffffff',
    flexShrink: 0,
  },
  logoMarkHero: {
    width: '52px',
    height: '52px',
  },
  heroBrandRow: {
    display: 'flex',
    alignItems: 'center',
    gap: '14px',
  },
  heroBrandName: {
    fontSize: '22px',
    fontWeight: 600,
    letterSpacing: '-0.374px',
  },
  heroTitle: {
    margin: 0,
    fontSize: '56px',
    fontWeight: 700,
    letterSpacing: '-0.88px',
    lineHeight: 1.05,
  },
  heroSubtitle: {
    margin: 0,
    fontSize: '17px',
    letterSpacing: '-0.28px',
    lineHeight: 1.5,
    color: LOGIN_COLORS.heroMuted,
    maxWidth: '420px',
  },
});

export function loginInputFocusStyle(focused: boolean): CSSProperties | undefined {
  if (!focused) return undefined;
  return {
    borderColor: LOGIN_COLORS.accent,
    boxShadow: '0 0 0 3px rgba(0, 113, 227, 0.3)',
  };
}
