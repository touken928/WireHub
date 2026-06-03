import { API_BASE, TOKEN_STORAGE_KEY } from '@/constants';
import type { PeerStatus, Settings } from '@/api/types';

export const STATUS_WS_PATH = '/ws/status';

export interface StatusMessage {
  type: 'status';
  peers: PeerStatus[];
  settings: Settings;
}

export type StatusListener = (msg: StatusMessage) => void;

function wsBase(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${window.location.host}${API_BASE}`;
}

export function statusWebSocketUrl(token: string): string {
  const q = new URLSearchParams({ token });
  return `${wsBase()}${STATUS_WS_PATH}?${q}`;
}

export function readStoredToken(): string | null {
  return localStorage.getItem(TOKEN_STORAGE_KEY);
}

export class StatusSocket {
  private ws: WebSocket | null = null;
  private listeners = new Set<StatusListener>();
  private reconnectMs = 1000;
  private stopped = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  connect(token: string) {
    this.stopped = false;
    this.open(token);
  }

  close() {
    this.stopped = true;
    if (this.reconnectTimer != null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  subscribe(listener: StatusListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private open(token: string) {
    if (this.stopped) return;
    const url = statusWebSocketUrl(token);
    const ws = new WebSocket(url);
    this.ws = ws;

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(String(ev.data)) as StatusMessage;
        if (msg.type !== 'status') return;
        this.reconnectMs = 1000;
        for (const fn of this.listeners) {
          fn(msg);
        }
      } catch {
        /* ignore malformed frames */
      }
    };

    ws.onclose = () => {
      this.ws = null;
      if (!this.stopped) {
        this.scheduleReconnect(token);
      }
    };

    ws.onerror = () => {
      ws.close();
    };
  }

  private scheduleReconnect(token: string) {
    if (this.reconnectTimer != null) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.reconnectMs = Math.min(this.reconnectMs * 2, 30_000);
      this.open(token);
    }, this.reconnectMs);
  }
}
