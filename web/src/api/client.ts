const API_BASE = '/api';

export const DNS_DOMAIN = 'wirehub';

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
  listen_port: number;
  mtu: number;
  status_interval: number;
  upstream_dns: string[];
}

export interface SetupStatus {
  configured: boolean;
  defaults: SetupDefaults;
}

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

export interface HubSettings {
  endpoint: string;
  subnet: string;
  admin_username: string;
  hub_ip: string;
  dns_ip: string;
  dns_suffix: string;
  listen_port: number;
  server_public_key: string;
  mtu: number;
  status_interval: number;
  upstream_dns: string[];
}

export interface PeerGroup {
  id: number;
  name: string;
  pos_x: number;
  pos_y: number;
  member_count: number;
}

export interface GroupLink {
  id: number;
  from_group_id: number;
  to_group_id: number;
}

export interface GroupGraphPeer {
  id: number;
  name: string;
  wg_ip: string;
  group_id: number;
  enabled: boolean;
  fqdn: string;
}

export interface GroupGraphNode {
  id: number;
  name: string;
  pos_x: number;
  pos_y: number;
  member_count: number;
  peers: GroupGraphPeer[];
}

export interface GroupGraph {
  groups: GroupGraphNode[];
  links: GroupLink[];
}

export interface Peer {
  id: number;
  name: string;
  public_key: string;
  wg_ip: string;
  group_id: number;
  group_name?: string;
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
  group_id: number;
  group_name: string;
  enabled: boolean;
  last_handshake: number;
  rx_bytes: number;
  tx_bytes: number;
  online: boolean;
}

export const api = {
  getSetupStatus: () => request<SetupStatus>('/setup/status'),

  importDatabase: async (file: File) => {
    const form = new FormData();
    form.append('database', file);
    const res = await fetch(`${API_BASE}/setup/import`, {
      method: 'POST',
      body: form,
    });
    const data = await res.json();
    if (!res.ok) {
      throw new Error(data.error || 'Import failed');
    }
    return data as { ok: boolean };
  },

  setup: (body: {
    endpoint: string;
    subnet?: string;
    admin_username?: string;
    admin_password: string;
    listen_port?: number;
    mtu?: number;
    status_interval?: number;
    upstream_dns?: string[];
  }) =>
    request<{ token: string }>('/setup', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  reset: (password: string) =>
    request<{ ok: boolean }>('/admin/reset', {
      method: 'POST',
      body: JSON.stringify({ password }),
    }),

  login: (username: string, password: string) =>
    request<{ token: string }>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  getStatus: () => request<{ peers: PeerStatus[]; settings: Settings }>('/status'),

  getSettings: () => request<HubSettings>('/settings'),

  updateSettings: (body: {
    listen_port: number;
    mtu: number;
    status_interval: number;
    upstream_dns: string[];
  }) =>
    request<{ ok: boolean; restart_required?: boolean }>('/settings', {
      method: 'PUT',
      body: JSON.stringify(body),
    }),

  changePassword: (current_password: string, new_password: string) =>
    request<{ ok: boolean }>('/settings/password', {
      method: 'PUT',
      body: JSON.stringify({ current_password, new_password }),
    }),

  exportDatabase: async () => {
    const headers: Record<string, string> = {};
    const token = getToken();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    const res = await fetch(`${API_BASE}/settings/export`, { headers });
    if (res.status === 401) {
      clearToken();
      window.location.href = '/login';
      throw new Error('Unauthorized');
    }
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      throw new Error(data.error || 'Export failed');
    }
    return res.blob();
  },

  listGroups: () => request<PeerGroup[]>('/groups'),
  createGroup: (body: { name: string; pos_x?: number; pos_y?: number }) =>
    request<PeerGroup>('/groups', { method: 'POST', body: JSON.stringify(body) }),
  updateGroup: (id: number, body: { name?: string; pos_x?: number; pos_y?: number }) =>
    request<PeerGroup>(`/groups/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deleteGroup: (id: number) =>
    request<{ ok: boolean }>(`/groups/${id}`, { method: 'DELETE' }),
  getGroupGraph: () => request<GroupGraph>('/groups/graph'),
  createGroupLink: (from_group_id: number, to_group_id: number) =>
    request<{ ok: boolean }>('/groups/links', {
      method: 'POST',
      body: JSON.stringify({ from_group_id, to_group_id }),
    }),
  deleteGroupLink: (from_group_id: number, to_group_id: number) =>
    request<{ ok: boolean }>('/groups/links', {
      method: 'DELETE',
      body: JSON.stringify({ from_group_id, to_group_id }),
    }),
  updateGroupLayout: (groups: { id: number; pos_x: number; pos_y: number }[]) =>
    request<{ ok: boolean }>('/groups/layout', {
      method: 'PUT',
      body: JSON.stringify({ groups }),
    }),

  listPeers: () => request<Peer[]>('/peers'),
  createPeer: (body: { name: string; group_id: number }) =>
    request<Peer>('/peers', { method: 'POST', body: JSON.stringify(body) }),
  updatePeer: (id: number, body: { group_id: number }) =>
    request<Peer>(`/peers/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deletePeer: (id: number) =>
    request<{ ok: boolean }>(`/peers/${id}`, { method: 'DELETE' }),
  togglePeer: (id: number) =>
    request<Peer>(`/peers/${id}/toggle`, { method: 'POST' }),
  getPeerConfig: (id: number) =>
    request<{ config: string; filename: string }>(`/peers/${id}/config`),
};

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatRate(bps: number): string {
  if (bps < 1024) return `${bps.toFixed(0)} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
  return `${(bps / 1024 / 1024).toFixed(1)} MB/s`;
}

export function formatHandshake(ts: number): string {
  if (!ts) return 'Never';
  return new Date(ts * 1000).toLocaleString();
}
