import { API_BASE } from '@/constants';
import { request, requestBlob, getSetupToken } from '@/api/http';
import type {
  GroupGraph,
  HubSettings,
  Peer,
  PeerGroup,
  PortForward,
  PortForwardList,
  MapList,
  ServiceMap,
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
    const setupToken = getSetupToken();
    const path = setupToken
      ? `${API_BASE}/setup/import?setup_token=${encodeURIComponent(setupToken)}`
      : `${API_BASE}/setup/import`;
    const res = await fetch(path, {
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
    request<{ ok: boolean; setup_token?: string }>('/admin/reset', {
      method: 'POST',
      body: JSON.stringify({ password }),
    }),

  login: (username: string, password: string) =>
    request<{ token: string }>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

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
  updateGroup: (id: number, body: { name?: string; pos_x?: number; pos_y?: number; allow_intra_group?: boolean }) =>
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
    },
  ) =>
    request<PortForward>(`/forwards/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deletePortForward: (id: number) =>
    request<{ ok: boolean }>(`/forwards/${id}`, { method: 'DELETE' }),

  listMaps: () => request<MapList>('/maps'),
  createMap: (body: {
    name?: string;
    slug: string;
    target_host: string;
    allowed_group_ids: number[];
  }) => request<ServiceMap>('/maps', { method: 'POST', body: JSON.stringify(body) }),
  updateMap: (
    id: number,
    body: {
      name?: string;
      slug: string;
      target_host: string;
      allowed_group_ids: number[];
    },
  ) => request<ServiceMap>(`/maps/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deleteMap: (id: number) => request<{ ok: boolean }>(`/maps/${id}`, { method: 'DELETE' }),

  listPeers: () => request<Peer[]>('/peers'),
  createPeer: (body: { name: string; group_id: number }) =>
    request<Peer>('/peers', { method: 'POST', body: JSON.stringify(body) }),
  updatePeer: (id: number, body: { group_id?: number; name?: string }) =>
    request<Peer>(`/peers/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deletePeer: (id: number) =>
    request<{ ok: boolean }>(`/peers/${id}`, { method: 'DELETE' }),
  togglePeer: (id: number) =>
    request<Peer>(`/peers/${id}/toggle`, { method: 'POST' }),
  getPeerConfig: (id: number) =>
    request<{ config: string; filename: string }>(`/peers/${id}/config`),
};
