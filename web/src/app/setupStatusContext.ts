import { createContext, useContext } from 'react';
import type { SetupStatus } from '@/api/types';

export type SetupStatusContextValue = {
  configured: boolean | null;
  refresh: () => Promise<SetupStatus>;
};

export const SetupStatusContext = createContext<SetupStatusContextValue | null>(null);

export function useSetupStatus(): SetupStatusContextValue {
  const ctx = useContext(SetupStatusContext);
  if (!ctx) {
    throw new Error('useSetupStatus must be used within SetupStatusProvider');
  }
  return ctx;
}
