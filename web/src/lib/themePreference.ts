import { THEME_STORAGE_KEY } from '@/constants';

export function loadThemeDark(): boolean {
  try {
    return localStorage.getItem(THEME_STORAGE_KEY) === 'dark';
  } catch {
    return false;
  }
}

export function saveThemeDark(dark: boolean) {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, dark ? 'dark' : 'light');
  } catch {
    // Ignore quota / private browsing errors.
  }
}
