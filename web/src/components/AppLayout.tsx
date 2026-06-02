import {
  makeStyles,
  tokens,
  Text,
  Button,
  Switch,
  Tooltip,
} from '@fluentui/react-components';
import {
  BoardRegular,
  PeopleTeamRegular,
  PersonRegular,
  SettingsRegular,
  SignOutRegular,
} from '@fluentui/react-icons';
import { useEffect } from 'react';
import { NavLink, Outlet, useLocation } from 'react-router-dom';
import { clearToken } from '../api/client';
import { LAYOUT } from '../styles/layout';

const useStyles = makeStyles({
  shell: {
    display: 'flex',
    minHeight: '100vh',
    backgroundColor: tokens.colorNeutralBackground2,
  },
  shellFill: {
    height: '100vh',
    minHeight: 'unset',
    overflow: 'hidden',
  },
  sidebar: {
    width: '220px',
    flexShrink: 0,
    padding: '20px 12px',
    borderRight: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    display: 'flex',
    flexDirection: 'column',
    gap: '8px',
  },
  brand: {
    fontSize: tokens.fontSizeBase500,
    fontWeight: tokens.fontWeightSemibold,
    color: tokens.colorBrandForeground1,
    padding: '4px 12px 16px',
  },
  navLink: {
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
    padding: '10px 12px',
    borderRadius: tokens.borderRadiusMedium,
    textDecoration: 'none',
    color: tokens.colorNeutralForeground1,
    ':hover': {
      backgroundColor: tokens.colorNeutralBackground1Hover,
    },
  },
  navActive: {
    backgroundColor: tokens.colorBrandBackground2,
    color: tokens.colorBrandForeground1,
    fontWeight: tokens.fontWeightSemibold,
  },
  main: {
    flex: 1,
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
  },
  mainFill: {
    minHeight: 0,
    overflow: 'hidden',
  },
  topBar: {
    display: 'flex',
    justifyContent: 'flex-end',
    alignItems: 'center',
    gap: '8px',
    padding: '6px 16px',
    minHeight: '40px',
    borderBottom: `1px solid ${tokens.colorNeutralStroke2}`,
    backgroundColor: tokens.colorNeutralBackground1,
    flexShrink: 0,
  },
  content: {
    flex: 1,
    padding: '16px 20px',
    overflow: 'visible',
  },
  contentFill: {
    display: 'flex',
    flexDirection: 'column',
    minHeight: 0,
    overflow: 'hidden',
  },
  contentInner: {
    width: '100%',
    maxWidth: LAYOUT.pageMaxWidth,
    marginLeft: 'auto',
    marginRight: 'auto',
    minWidth: 0,
    overflow: 'visible',
  },
  contentInnerFill: {
    flex: 1,
    display: 'flex',
    flexDirection: 'column',
    minHeight: 0,
    overflow: 'hidden',
  },
  sidebarFooter: {
    marginTop: 'auto',
    paddingTop: '16px',
  },
  compactSwitch: {
    '& label': {
      fontSize: tokens.fontSizeBase200,
    },
  },
});

interface AppLayoutProps {
  dark: boolean;
  onToggleTheme: () => void;
}

export default function AppLayout({ dark, onToggleTheme }: AppLayoutProps) {
  const styles = useStyles();
  const location = useLocation();
  const isFill = location.pathname.startsWith('/groups');

  useEffect(() => {
    document.documentElement.classList.toggle('layout-fill', isFill);
    return () => document.documentElement.classList.remove('layout-fill');
  }, [isFill]);

  const navClass = (path: string) => {
    const active = path === '/' ? location.pathname === '/' : location.pathname.startsWith(path);
    return `${styles.navLink} ${active ? styles.navActive : ''}`;
  };

  return (
    <div className={`${styles.shell} ${isFill ? styles.shellFill : ''}`}>
      <aside className={styles.sidebar}>
        <Text className={styles.brand}>WireHub</Text>
        <NavLink to="/" className={navClass('/')}>
          <BoardRegular /> Dashboard
        </NavLink>
        <NavLink to="/groups" className={navClass('/groups')}>
          <PeopleTeamRegular /> Groups
        </NavLink>
        <NavLink to="/users" className={navClass('/users')}>
          <PersonRegular /> Users
        </NavLink>
        <NavLink to="/settings" className={navClass('/settings')}>
          <SettingsRegular /> Settings
        </NavLink>
        <div className={styles.sidebarFooter}>
          <Text size={200} style={{ color: tokens.colorNeutralForeground3, padding: '0 12px' }}>
            Hub-and-spoke VPN
          </Text>
        </div>
      </aside>
      <div className={`${styles.main} ${isFill ? styles.mainFill : ''}`}>
        <div className={styles.topBar}>
          <Switch
            className={styles.compactSwitch}
            label={dark ? 'Dark' : 'Light'}
            checked={dark}
            onChange={onToggleTheme}
          />
          <Tooltip content="Logout" relationship="label">
            <Button
              size="small"
              appearance="subtle"
              icon={<SignOutRegular />}
              onClick={() => {
                clearToken();
                window.location.href = '/login';
              }}
            />
          </Tooltip>
        </div>
        <div className={`${styles.content} ${isFill ? styles.contentFill : ''}`}>
          <div className={`${styles.contentInner} ${isFill ? styles.contentInnerFill : ''}`}>
            <Outlet />
          </div>
        </div>
      </div>
    </div>
  );
}
