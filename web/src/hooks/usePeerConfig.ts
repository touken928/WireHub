import { useCallback, useState } from 'react';
import { api } from '@/api';
import { getErrorMessage } from '@/lib/error';

export function usePeerConfig() {
  const [open, setOpen] = useState(false);
  const [config, setConfig] = useState('');
  const [filename, setFilename] = useState('peer.conf');

  const showConfig = useCallback(async (peerId: number) => {
    const { config: text, filename: name } = await api.getPeerConfig(peerId);
    setConfig(text);
    setFilename(name);
    setOpen(true);
  }, []);

  const close = useCallback(() => setOpen(false), []);

  return { open, config, filename, showConfig, close };
}

export async function runPeerAction(
  action: () => Promise<void>,
  onError?: (message: string) => void,
) {
  try {
    await action();
  } catch (error) {
    const message = getErrorMessage(error, 'Operation failed');
    if (onError) onError(message);
    else alert(message);
  }
}
