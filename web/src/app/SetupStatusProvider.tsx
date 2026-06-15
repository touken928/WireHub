import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from '@/api';
import { ApiError } from '@/api/http';
import type { SetupStatus } from '@/api/types';
import { SetupStatusContext } from '@/app/setupStatusContext';

export function SetupStatusProvider({ children }: { children: ReactNode }) {
  const [configured, setConfigured] = useState<boolean | null>(null);

  const refresh = useCallback(async (): Promise<SetupStatus> => {
    try {
      const status = await api.getSetupStatus();
      setConfigured(status.configured);
      return status;
    } catch (err) {
      setConfigured(err instanceof ApiError && err.status === 403 ? false : true);
      throw err;
    }
  }, []);

  useEffect(() => {
    void refresh().catch(() => {});
  }, [refresh]);

  const value = useMemo(
    () => ({ configured, refresh }),
    [configured, refresh],
  );

  return (
    <SetupStatusContext.Provider value={value}>{children}</SetupStatusContext.Provider>
  );
}
