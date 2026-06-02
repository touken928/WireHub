import {
  FluentProvider,
  webDarkTheme,
  webLightTheme,
} from '@fluentui/react-components';
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { ThemeContext } from '@/app/themeContext';
import { loadThemeDark, saveThemeDark } from '@/lib/themePreference';

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [dark, setDark] = useState(loadThemeDark);

  useEffect(() => {
    saveThemeDark(dark);
  }, [dark]);

  const toggleTheme = useCallback(() => setDark((value) => !value), []);
  const value = useMemo(() => ({ dark, toggleTheme }), [dark, toggleTheme]);

  return (
    <ThemeContext.Provider value={value}>
      <FluentProvider theme={dark ? webDarkTheme : webLightTheme}>
        {children}
      </FluentProvider>
    </ThemeContext.Provider>
  );
}
