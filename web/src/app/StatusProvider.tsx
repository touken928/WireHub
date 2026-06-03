import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import type { PeerStatus, Settings } from '@/api/types';
import { readStoredToken, StatusSocket, type StatusMessage } from '@/ws/status';

interface StatusContextValue {
  peers: PeerStatus[];
  settings: Settings | null;
  connected: boolean;
}

const StatusContext = createContext<StatusContextValue | null>(null);

export function StatusProvider({ children }: { children: ReactNode }) {
  const [peers, setPeers] = useState<PeerStatus[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [connected, setConnected] = useState(false);

  const onMessage = useCallback((msg: StatusMessage) => {
    setPeers(msg.peers ?? []);
    setSettings(msg.settings ?? null);
    setConnected(true);
  }, []);

  useEffect(() => {
    const token = readStoredToken();
    if (!token) return;

    const socket = new StatusSocket();
    socket.subscribe(onMessage);
    socket.connect(token);

    return () => socket.close();
  }, [onMessage]);

  const value = useMemo(
    () => ({ peers, settings, connected }),
    [peers, settings, connected],
  );

  return (
    <StatusContext.Provider value={value}>{children}</StatusContext.Provider>
  );
}

export function useStatus(): StatusContextValue {
  const ctx = useContext(StatusContext);
  if (!ctx) {
    throw new Error('useStatus must be used within StatusProvider');
  }
  return ctx;
}
