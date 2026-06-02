import {
  FluentProvider,
  webLightTheme,
  webDarkTheme,
  makeStyles,
  tokens,
  Spinner,
} from '@fluentui/react-components';
import { BrowserRouter, Routes, Route, Navigate, Outlet, useLocation } from 'react-router-dom';
import { lazy, Suspense, useEffect, useState } from 'react';
import { api, getToken } from './api/client';

const LoginPage = lazy(() => import('./pages/LoginPage'));
const SetupPage = lazy(() => import('./pages/SetupPage'));
const HomePage = lazy(() => import('./pages/HomePage'));

const useStyles = makeStyles({
  root: {
    minHeight: '100vh',
    backgroundColor: tokens.colorNeutralBackground2,
  },
});

function PrivateRoute() {
  if (!getToken()) {
    return <Navigate to="/login" replace />;
  }
  return <Outlet />;
}

function SetupGate({ children }: { children: React.ReactNode }) {
  const location = useLocation();
  const [configured, setConfigured] = useState<boolean | null>(null);

  useEffect(() => {
    api.getSetupStatus()
      .then((status) => setConfigured(status.configured))
      .catch(() => setConfigured(true));
  }, [location.pathname]);

  if (configured === null) {
    return <Spinner label="Loading..." />;
  }

  if (!configured && location.pathname !== '/setup') {
    return <Navigate to="/setup" replace />;
  }
  if (configured && location.pathname === '/setup') {
    return <Navigate to={getToken() ? '/' : '/login'} replace />;
  }

  return children;
}

export default function App() {
  const styles = useStyles();
  const [dark, setDark] = useState(false);

  return (
    <FluentProvider theme={dark ? webDarkTheme : webLightTheme}>
      <div className={styles.root}>
        <BrowserRouter>
          <SetupGate>
            <Suspense fallback={<Spinner label="Loading..." />}>
              <Routes>
                <Route path="/setup" element={<SetupPage />} />
                <Route path="/login" element={<LoginPage />} />
                <Route element={<PrivateRoute />}>
                  <Route
                    index
                    element={
                      <HomePage dark={dark} onToggleTheme={() => setDark(!dark)} />
                    }
                  />
                </Route>
                <Route path="*" element={<Navigate to="/" replace />} />
              </Routes>
            </Suspense>
          </SetupGate>
        </BrowserRouter>
      </div>
    </FluentProvider>
  );
}
