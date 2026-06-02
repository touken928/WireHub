import { useCallback, useState, type ReactNode } from 'react';
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Text,
} from '@fluentui/react-components';
import { ConfirmContext } from '@/components/common/confirmContext';
import type { ConfirmOptions } from '@/components/common/confirmTypes';

type PendingConfirm = ConfirmOptions & {
  resolve: (confirmed: boolean) => void;
};

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<PendingConfirm | null>(null);

  const confirm = useCallback((options: ConfirmOptions) => {
    return new Promise<boolean>((resolve) => {
      setPending({ ...options, resolve });
    });
  }, []);

  const finish = (confirmed: boolean) => {
    pending?.resolve(confirmed);
    setPending(null);
  };

  return (
    <ConfirmContext.Provider value={{ confirm }}>
      {children}
      <Dialog
        open={pending != null}
        onOpenChange={(_, data) => {
          if (!data.open) finish(false);
        }}
      >
        <DialogSurface>
          <DialogBody>
            <DialogTitle>{pending?.title}</DialogTitle>
            <DialogContent>
              <Text>{pending?.message}</Text>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => finish(false)}>Cancel</Button>
              <Button appearance="primary" onClick={() => finish(true)}>
                {pending?.confirmLabel ?? 'Confirm'}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </ConfirmContext.Provider>
  );
}
