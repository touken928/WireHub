import { API_BASE } from '@/constants';
import { clearToken, getToken } from '@/api/auth';

const SETUP_TOKEN_KEY = 'wirehub_setup_token';

export class ApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

/** Reads the setup token — URL ?setup_token= always wins over sessionStorage.
 *  This ensures a fresh token from a reset redirect replaces any stale stored value. */
export function getSetupToken(): string | null {
  // URL first: after a hub reset the fresh token arrives via query param.
  const params = new URLSearchParams(window.location.search);
  const fromUrl = params.get('setup_token');
  if (fromUrl) {
    sessionStorage.setItem(SETUP_TOKEN_KEY, fromUrl);
    // Clean the token out of the address bar once captured.
    params.delete('setup_token');
    const nextSearch = params.toString();
    const nextUrl = `${window.location.pathname}${nextSearch ? `?${nextSearch}` : ''}${window.location.hash ?? ''}`;
    window.history.replaceState(null, '', nextUrl);
    return fromUrl;
  }
  // Fall back to sessionStorage (captured on an earlier page load or URL visit).
  return sessionStorage.getItem(SETUP_TOKEN_KEY);
}

/** Clears the stored setup token (e.g. after successful setup). */
export function clearSetupToken(): void {
  sessionStorage.removeItem(SETUP_TOKEN_KEY);
}

function withSetupToken(path: string): string {
  const setupToken = getSetupToken();
  if (!setupToken) return `${API_BASE}${path}`;
  const separator = path.includes('?') ? '&' : '?';
  return `${API_BASE}${path}${separator}setup_token=${encodeURIComponent(setupToken)}`;
}

async function fetchSetupConfigured(): Promise<boolean> {
  try {
    const res = await fetch(withSetupToken('/setup/status'));
    // 403 from /setup/status means the hub is definitively unconfigured:
    // a configured hub bypasses the setup-token check entirely.
    if (res.status === 403) return false;
    if (!res.ok) return true;
    const data = (await res.json()) as { configured?: boolean };
    return data.configured !== false;
  } catch {
    return true;
  }
}

async function redirectOnUnauthorized() {
  const configured = await fetchSetupConfigured();
  window.location.href = configured ? '/login' : '/setup';
}

export async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  };
  const token = getToken();
  const isLogin = path === '/auth/login';
  if (token && !isLogin) {
    headers.Authorization = `Bearer ${token}`;
  }

  const res = await fetch(withSetupToken(path), { ...options, headers });

  if (res.status === 401) {
    const data = await res.json().catch(() => ({} as { error?: string }));
    if (isLogin) {
      throw new ApiError(data.error || 'Invalid credentials', 401);
    }
    clearToken();
    await redirectOnUnauthorized();
    throw new ApiError('Unauthorized', 401);
  }

  const data = await res.json();
  if (!res.ok) {
    if (res.status === 429) {
      const retry = res.headers.get('Retry-After');
      const msg = data.error || 'Too many login attempts';
      throw new ApiError(retry ? `${msg} (retry after ${retry}s)` : msg, 429);
    }
    throw new ApiError(data.error || 'Request failed', res.status);
  }
  return data as T;
}

export async function requestBlob(path: string): Promise<Blob> {
  const headers: Record<string, string> = {};
  const token = getToken();
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  const res = await fetch(`${API_BASE}${path}`, { headers });

  if (res.status === 401) {
    clearToken();
    await redirectOnUnauthorized();
    throw new ApiError('Unauthorized', 401);
  }

  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || 'Request failed');
  }

  return res.blob();
}
