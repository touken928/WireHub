import { Spinner } from '@fluentui/react-components';
import { lazy, Suspense } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import { AppLayout } from '@/components/layout/AppLayout';
import { RequireAuth } from '@/app/guards/RequireAuth';

const LoginPage = lazy(() => import('@/pages/LoginPage'));
const SetupPage = lazy(() => import('@/pages/SetupPage'));
const DashboardPage = lazy(() => import('@/pages/DashboardPage'));
const GroupsPage = lazy(() => import('@/pages/GroupsPage'));
const UsersPage = lazy(() => import('@/pages/UsersPage'));
const ForwardPage = lazy(() => import('@/pages/ForwardPage'));
const SettingsPage = lazy(() => import('@/pages/SettingsPage'));

export function AppRoutes() {
  return (
    <Suspense fallback={<Spinner label="Loading..." />}>
      <Routes>
        <Route path="/setup" element={<SetupPage />} />
        <Route path="/login" element={<LoginPage />} />
        <Route
          path="/"
          element={
            <RequireAuth>
              <AppLayout />
            </RequireAuth>
          }
        >
          <Route index element={<DashboardPage />} />
          <Route path="groups" element={<GroupsPage />} />
          <Route path="users" element={<UsersPage />} />
          <Route path="forward" element={<ForwardPage />} />
          <Route path="settings" element={<SettingsPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Suspense>
  );
}
