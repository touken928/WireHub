import { useEffect } from 'react';

/** Runs `load` on mount and on a fixed interval until unmount. */
export function usePolling(load: () => void | Promise<void>, intervalMs: number) {
  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), intervalMs);
    return () => clearInterval(timer);
  }, [load, intervalMs]);
}
