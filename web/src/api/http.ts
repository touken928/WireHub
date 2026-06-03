import { API_BASE } from '@/constants';
import { clearToken, getToken } from '@/api/auth';

async function redirectOnUnauthorized() {
  const setup = await fetch(`${API_BASE}/setup/status`)
    .then((r) => r.json())
    .catch(() => ({ configured: true }));
  window.location.href = setup.configured ? '/login' : '/setup';
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

  const res = await fetch(`${API_BASE}${path}`, { ...options, headers });

  if (res.status === 401) {
    const data = await res.json().catch(() => ({} as { error?: string }));
    if (isLogin) {
      throw new Error(data.error || 'Invalid credentials');
    }
    clearToken();
    await redirectOnUnauthorized();
    throw new Error('Unauthorized');
  }

  const data = await res.json();
  if (!res.ok) {
    if (res.status === 429) {
      const retry = res.headers.get('Retry-After');
      const msg = data.error || 'Too many login attempts';
      throw new Error(retry ? `${msg} (retry after ${retry}s)` : msg);
    }
    throw new Error(data.error || 'Request failed');
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
    window.location.href = '/login';
    throw new Error('Unauthorized');
  }

  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || 'Request failed');
  }

  return res.blob();
}
