import {
  FluentProvider,
  webLightTheme,
  webDarkTheme,
  Spinner,
} from '@fluentui/react-components';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { lazy, Suspense, useEffect, useState } from 'react';
import { api, getToken } from './api/client';
import AppLayout from './components/AppLayout';
import { ConfirmProvider } from './components/ConfirmContext';

const LoginPage = lazy(() => import('./pages/LoginPage'));
const SetupPage = lazy(() => import('./pages/SetupPage'));
const DashboardPage = lazy(() => import('./pages/DashboardPage'));
const GroupsPage = lazy(() => import('./pages/GroupsPage'));
const UsersPage = lazy(() => import('./pages/UsersPage'));
const SettingsPage = lazy(() => import('./pages/SettingsPage'));

function SetupGate({ children }: { children: React.ReactNode }) {
  const [configured, setConfigured] = useState<boolean | null>(null);
  const path = window.location.pathname;

  useEffect(() => {
    api.getSetupStatus()
      .then((status) => setConfigured(status.configured))
      .catch(() => setConfigured(true));
  }, []);

  if (configured === null) {
    return <Spinner label="Loading..." />;
  }
  if (!configured && path !== '/setup') {
    return <Navigate to="/setup" replace />;
  }
  if (configured && path === '/setup') {
    return <Navigate to={getToken() ? '/' : '/login'} replace />;
  }
  return children;
}

function RequireAuth({ children }: { children: React.ReactNode }) {
  if (!getToken()) {
    return <Navigate to="/login" replace />;
  }
  return children;
}

function AppShell() {
  const [dark, setDark] = useState(false);

  return (
    <FluentProvider theme={dark ? webDarkTheme : webLightTheme}>
      <ConfirmProvider>
        <AppLayout dark={dark} onToggleTheme={() => setDark(!dark)} />
      </ConfirmProvider>
    </FluentProvider>
  );
}

export default function App() {
  return (
    <FluentProvider theme={webLightTheme}>
      <BrowserRouter>
        <SetupGate>
          <Suspense fallback={<Spinner label="Loading..." />}>
            <Routes>
              <Route path="/setup" element={<SetupPage />} />
              <Route path="/login" element={<LoginPage />} />
              <Route
                path="/"
                element={
                  <RequireAuth>
                    <AppShell />
                  </RequireAuth>
                }
              >
                <Route index element={<DashboardPage />} />
                <Route path="groups" element={<GroupsPage />} />
                <Route path="users" element={<UsersPage />} />
                <Route path="settings" element={<SettingsPage />} />
              </Route>
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </Suspense>
        </SetupGate>
      </BrowserRouter>
    </FluentProvider>
  );
}
