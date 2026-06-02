const API_BASE = '/api';

export const DNS_DOMAIN = 'wirehub.internal';

export function getToken(): string | null {
  return localStorage.getItem('wirehub_token');
}

export function setToken(token: string) {
  localStorage.setItem('wirehub_token', token);
}

export function clearToken() {
  localStorage.removeItem('wirehub_token');
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  };
  const token = getToken();
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }
  const res = await fetch(`${API_BASE}${path}`, { ...options, headers });
  if (res.status === 401) {
    clearToken();
    const setup = await fetch(`${API_BASE}/setup/status`).then((r) => r.json()).catch(() => ({ configured: true }));
    window.location.href = setup.configured ? '/login' : '/setup';
    throw new Error('Unauthorized');
  }
  const data = await res.json();
  if (!res.ok) {
    throw new Error(data.error || 'Request failed');
  }
  return data as T;
}

export interface SetupDefaults {
  subnet: string;
  admin_username: string;
  mtu: number;
  status_interval: number;
  upstream_dns: string[];
}

export interface SetupStatus {
  configured: boolean;
  defaults: SetupDefaults;
}

export const api = {
  getSetupStatus: () => request<SetupStatus>('/setup/status'),

  setup: (body: {
    endpoint: string;
    subnet?: string;
    admin_username?: string;
    admin_password: string;
    mtu?: number;
    status_interval?: number;
    upstream_dns?: string[];
  }) =>
    request<{ token: string }>('/setup', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  reset: () => request<{ ok: boolean }>('/admin/reset', { method: 'POST' }),

  login: (username: string, password: string) =>
    request<{ token: string }>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  getStatus: () => request<{ peers: PeerStatus[]; settings: Settings }>('/status'),

  listPeers: () => request<Peer[]>('/peers'),
  createPeer: (body: CreatePeerBody) =>
    request<Peer>('/peers', { method: 'POST', body: JSON.stringify(body) }),
  updatePeer: (id: number, body: UpdatePeerBody) =>
    request<Peer>(`/peers/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deletePeer: (id: number) =>
    request<{ ok: boolean }>(`/peers/${id}`, { method: 'DELETE' }),
  togglePeer: (id: number) =>
    request<Peer>(`/peers/${id}/toggle`, { method: 'POST' }),
  getPeerConfig: (id: number) =>
    request<{ config: string; filename: string }>(`/peers/${id}/config`),
};

export interface Settings {
  server_public_key: string;
  endpoint: string;
  listen_port: number;
  wg_subnet: string;
  hub_ip: string;
  dns_ip: string;
  dns_suffix: string;
  upstream_dns?: string[];
}

export interface Peer {
  id: number;
  name: string;
  public_key: string;
  wg_ip: string;
  access_exclude: string[];
  enabled: boolean;
  fqdn: string;
  last_handshake: number;
  rx_bytes: number;
  tx_bytes: number;
  created_at: string;
}

export interface PeerStatus {
  id: number;
  name: string;
  fqdn: string;
  wg_ip: string;
  access_exclude: string[];
  enabled: boolean;
  last_handshake: number;
  rx_bytes: number;
  tx_bytes: number;
  online: boolean;
}

export interface CreatePeerBody {
  name: string;
  access_exclude?: string[];
}

export interface UpdatePeerBody {
  access_exclude?: string[];
}

export function formatRate(bps: number): string {
  if (bps < 1024) return `${Math.round(bps)} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
  return `${(bps / 1024 / 1024).toFixed(2)} MB/s`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatHandshake(ts: number): string {
  if (!ts) return 'Never';
  return new Date(ts * 1000).toLocaleString();
}
