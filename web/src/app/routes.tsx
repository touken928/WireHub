import { Navigate, Route, Routes } from 'react-router-dom';
import { AppLayout } from '@/components/layout/AppLayout';
import { RequireAuth } from '@/app/guards/RequireAuth';
import { StatusProvider } from '@/app/StatusProvider';
import DashboardPage from '@/pages/DashboardPage';
import ForwardPage from '@/pages/ForwardPage';
import GroupsPage from '@/pages/GroupsPage';
import LoginPage from '@/pages/LoginPage';
import PeersPage from '@/pages/PeersPage';
import SettingsPage from '@/pages/SettingsPage';
import SetupPage from '@/pages/SetupPage';

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/setup" element={<SetupPage />} />
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/"
        element={
          <RequireAuth>
            <StatusProvider>
              <AppLayout />
            </StatusProvider>
          </RequireAuth>
        }
      >
        <Route index element={<DashboardPage />} />
        <Route path="groups" element={<GroupsPage />} />
        <Route path="peers" element={<PeersPage />} />
        <Route path="forward" element={<ForwardPage />} />
        <Route path="settings" element={<SettingsPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
