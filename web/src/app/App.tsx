import { BrowserRouter } from 'react-router-dom';
import { ConfirmProvider } from '@/components/common/ConfirmProvider';
import { ThemeProvider } from '@/app/ThemeProvider';
import { SetupGate } from '@/app/guards/SetupGate';
import { AppRoutes } from '@/app/routes';

export default function App() {
  return (
    <ThemeProvider>
      <BrowserRouter>
        <ConfirmProvider>
          <SetupGate>
            <AppRoutes />
          </SetupGate>
        </ConfirmProvider>
      </BrowserRouter>
    </ThemeProvider>
  );
}
