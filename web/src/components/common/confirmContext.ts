import { createContext } from 'react';
import type { ConfirmOptions } from '@/components/common/confirmTypes';

export type ConfirmContextValue = {
  confirm: (options: ConfirmOptions) => Promise<boolean>;
};

export const ConfirmContext = createContext<ConfirmContextValue | null>(null);
