import {
  makeStyles,
  tokens,
  Text,
  Button,
  Tooltip,
} from '@fluentui/react-components';
import {
  BoardRegular,
  PeopleTeamRegular,
  PersonRegular,
  ArrowRoutingRegular,
  GlobeRegular,
  SettingsRegular,
  SignOutRegular,
  WeatherMoonRegular,
  WeatherSunnyRegular,
} from '@fluentui/react-icons';
import { useEffect } from 'react';
import { NavLink, Outlet, useLocation } from 'react-router-dom';
import { clearToken } from '@/api';
import { useTheme } from '@/app/useTheme';
import { RouteErrorBoundary } from '@/components/common/RouteErrorBoundary';
import { LAYOUT } from '@/styles/layout';

const useStyles = makeStyles({
  shell: {
    display: 'flex',
    height: '100vh',
    minHeight: '100vh',
    overflow: 'hidden',
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
    height: '100vh',
    overflowY: 'auto',
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
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    overflow: 'hidden',
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
    minHeight: 0,
    padding: '16px 20px',
    overflow: 'auto',
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
});

const NAV_ITEMS = [
  { path: '/', label: 'Dashboard', icon: BoardRegular, exact: true },
  { path: '/groups', label: 'Groups', icon: PeopleTeamRegular, exact: false },
  { path: '/peers', label: 'Peers', icon: PersonRegular, exact: false },
  { path: '/forward', label: 'Forward', icon: ArrowRoutingRegular, exact: false },
  { path: '/maps', label: 'Maps', icon: GlobeRegular, exact: false },
  { path: '/settings', label: 'Settings', icon: SettingsRegular, exact: false },
] as const;

const GITHUB_REPO_URL = 'https://github.com/touken928/wirehub';

function GitHubIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49C4 14.09 3.48 13.22 3.32 12.77c-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.5 7.5 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

export function AppLayout() {
  const styles = useStyles();
  const { dark, toggleTheme } = useTheme();
  const location = useLocation();
  const isFill = location.pathname.startsWith('/groups');

  useEffect(() => {
    document.documentElement.classList.add('app-shell');
    document.documentElement.classList.toggle('layout-fill', isFill);
    return () => {
      document.documentElement.classList.remove('app-shell', 'layout-fill');
    };
  }, [isFill]);

  const navClass = (path: string, exact: boolean) => {
    const active = exact ? location.pathname === path : location.pathname.startsWith(path);
    return `${styles.navLink} ${active ? styles.navActive : ''}`;
  };

  return (
    <div className={`${styles.shell} ${isFill ? styles.shellFill : ''}`}>
      <aside className={styles.sidebar}>
        <Text className={styles.brand}>WireHub</Text>
        {NAV_ITEMS.map(({ path, label, icon: Icon, exact }) => (
          <NavLink key={path} to={path} className={navClass(path, exact)}>
            <Icon /> {label}
          </NavLink>
        ))}
        <div className={styles.sidebarFooter}>
          <Text size={200} style={{ color: tokens.colorNeutralForeground3, padding: '0 12px' }}>
            Hub-and-spoke VPN
          </Text>
        </div>
      </aside>
      <div className={styles.main}>
        <div className={styles.topBar}>
          <Tooltip content="Star on GitHub" relationship="label">
            <Button
              size="small"
              appearance="subtle"
              icon={<GitHubIcon />}
              aria-label="Star on GitHub"
              onClick={() => window.open(GITHUB_REPO_URL, '_blank', 'noreferrer')}
            />
          </Tooltip>
          <Button
            size="small"
            appearance="subtle"
            icon={dark ? <WeatherSunnyRegular /> : <WeatherMoonRegular />}
            aria-label={dark ? 'Light mode' : 'Dark mode'}
            onClick={toggleTheme}
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
            <RouteErrorBoundary>
              <Outlet />
            </RouteErrorBoundary>
          </div>
        </div>
      </div>
    </div>
  );
}
