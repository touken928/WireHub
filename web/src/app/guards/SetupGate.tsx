import { Spinner } from '@fluentui/react-components';
import { type ReactNode } from 'react';
import { Navigate, useLocation } from 'react-router-dom';
import { getToken } from '@/api';
import { useSetupStatus } from '@/app/setupStatusContext';

export function SetupGate({ children }: { children: ReactNode }) {
  const { configured } = useSetupStatus();
  const { pathname } = useLocation();

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
