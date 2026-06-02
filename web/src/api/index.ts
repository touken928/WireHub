import { API_BASE } from '@/constants';
import { request, requestBlob } from '@/api/http';
import type {
  GroupGraph,
  HubSettings,
  Peer,
  PeerGroup,
  PeerStatus,
  PortForward,
  PortForwardList,
  Settings,
  SetupStatus,
} from '@/api/types';

export type * from '@/api/types';
export { clearToken, getToken, setToken } from '@/api/auth';
export { formatBytes, formatHandshake, formatRate } from '@/lib/format';

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

  exportDatabase: () => requestBlob('/settings/export'),

  listGroups: () => request<PeerGroup[]>('/groups'),
  createGroup: (body: { name: string; pos_x?: number; pos_y?: number }) =>
    request<PeerGroup>('/groups', { method: 'POST', body: JSON.stringify(body) }),
  updateGroup: (id: number, body: { name?: string; pos_x?: number; pos_y?: number }) =>
    request<PeerGroup>(`/groups/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deleteGroup: (id: number) =>
    request<{ ok: boolean }>(`/groups/${id}`, { method: 'DELETE' }),
  getGroupGraph: () => request<GroupGraph>('/groups/graph'),
  createGroupLink: (body: { from_group_id: number; to_group_id: number; bidirectional?: boolean }) =>
    request<{ ok: boolean }>('/groups/links', {
      method: 'POST',
      body: JSON.stringify(body),
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

  listPortForwards: () => request<PortForwardList>('/forwards'),
  createPortForward: (body: {
    name?: string;
    listen_port: number;
    protocol: string;
    target_host: string;
    target_port: number;
    enabled?: boolean;
  }) =>
    request<PortForward>('/forwards', { method: 'POST', body: JSON.stringify(body) }),
  updatePortForward: (
    id: number,
    body: {
      name?: string;
      listen_port: number;
      protocol: string;
      target_host: string;
      target_port: number;
      enabled?: boolean;
    },
  ) =>
    request<PortForward>(`/forwards/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deletePortForward: (id: number) =>
    request<{ ok: boolean }>(`/forwards/${id}`, { method: 'DELETE' }),

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
