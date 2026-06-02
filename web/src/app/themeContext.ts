import { createContext } from 'react';

export type ThemeContextValue = {
  dark: boolean;
  toggleTheme: () => void;
};

export const ThemeContext = createContext<ThemeContextValue | null>(null);
