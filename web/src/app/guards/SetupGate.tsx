import { Spinner } from '@fluentui/react-components';
import { useEffect, useState, type ReactNode } from 'react';
import { Navigate, useLocation } from 'react-router-dom';
import { api, getToken } from '@/api';

export function SetupGate({ children }: { children: ReactNode }) {
  const [configured, setConfigured] = useState<boolean | null>(null);
  const { pathname } = useLocation();

  useEffect(() => {
    api.getSetupStatus()
      .then((status) => setConfigured(status.configured))
      .catch(() => setConfigured(true));
  }, []);

  if (configured === null) {
    return <Spinner label="Loading..." />;
  }
  if (!configured && pathname !== '/setup') {
    return <Navigate to="/setup" replace />;
  }
  if (configured && pathname === '/setup') {
    return <Navigate to={getToken() ? '/' : '/login'} replace />;
  }
  return children;
}
