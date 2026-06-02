import { BrowserRouter } from 'react-router-dom';
import { ConfirmProvider } from '@/components/common/ConfirmProvider';
import { ThemeProvider } from '@/app/ThemeProvider';
import { SetupGate } from '@/app/guards/SetupGate';
import { AppRoutes } from '@/app/routes';
import { SetupStatusProvider } from '@/app/SetupStatusProvider';

export default function App() {
  return (
    <ThemeProvider>
      <BrowserRouter>
        <SetupStatusProvider>
          <ConfirmProvider>
            <SetupGate>
              <AppRoutes />
            </SetupGate>
          </ConfirmProvider>
        </SetupStatusProvider>
      </BrowserRouter>
    </ThemeProvider>
  );
}
