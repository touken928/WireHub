import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock auth module before any imports: hoisted by vitest
vi.mock('@/api/auth', () => ({
  getToken: vi.fn(),
  setToken: vi.fn(),
  clearToken: vi.fn(),
}));

const mockLocation: { href: string } = { href: '' };
let fetchMock: ReturnType<typeof vi.fn>;
const replaceStateMock = vi.fn();

beforeEach(() => {
  mockLocation.href = '';
  fetchMock = vi.fn();
  vi.stubGlobal('window', { location: mockLocation, history: { replaceState: replaceStateMock } });
  vi.stubGlobal('location', mockLocation);
  vi.stubGlobal('fetch', fetchMock);
  replaceStateMock.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function pushFetchResponse(status: number, body: unknown) {
  fetchMock.mockResolvedValueOnce({
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response);
}

describe('request 401 handling', () => {
  it('clears token and redirects to /login when hub is configured', async () => {
    pushFetchResponse(401, { error: 'unauthorized' });
    pushFetchResponse(200, { configured: true });

    const { getToken, clearToken } = await import('@/api/auth');
    vi.mocked(getToken).mockReturnValue('fake-token');

    const { request } = await import('@/api/http');
    await expect(request('/settings', { method: 'GET' })).rejects.toThrow('Unauthorized');
    expect(clearToken).toHaveBeenCalled();
    expect(mockLocation.href).toBe('/login');
  });

  it('redirects to /setup when hub is not configured', async () => {
    pushFetchResponse(401, { error: 'unauthorized' });
    pushFetchResponse(200, { configured: false });

    const { getToken } = await import('@/api/auth');
    vi.mocked(getToken).mockReturnValue('fake-token');

    const { request } = await import('@/api/http');
    await expect(request('/settings', { method: 'GET' })).rejects.toThrow('Unauthorized');
    expect(mockLocation.href).toBe('/setup');
  });

  it('does not redirect on login 401; throws credentials error', async () => {
    pushFetchResponse(401, { error: 'Invalid credentials' });

    const { request } = await import('@/api/http');
    await expect(request('/auth/login', { method: 'POST' })).rejects.toThrow('Invalid credentials');
    expect(mockLocation.href).toBe('');
  });
});

describe('requestBlob 401 handling', () => {
  it('clears token and redirects to /login when configured', async () => {
    pushFetchResponse(401, { error: 'unauthorized' });
    pushFetchResponse(200, { configured: true });

    const { getToken, clearToken } = await import('@/api/auth');
    vi.mocked(getToken).mockReturnValue('fake-token');

    const { requestBlob } = await import('@/api/http');
    await expect(requestBlob('/settings/export')).rejects.toThrow('Unauthorized');
    expect(clearToken).toHaveBeenCalled();
    expect(mockLocation.href).toBe('/login');
  });

  it('redirects to /setup when not configured', async () => {
    pushFetchResponse(401, { error: 'unauthorized' });
    pushFetchResponse(200, { configured: false });

    const { getToken } = await import('@/api/auth');
    vi.mocked(getToken).mockReturnValue('fake-token');

    const { requestBlob } = await import('@/api/http');
    await expect(requestBlob('/settings/export')).rejects.toThrow('Unauthorized');
    expect(mockLocation.href).toBe('/setup');
  });
});

describe('fetch error fallback', () => {
  it('redirects to /login when fetchSetupConfigured fails', async () => {
    pushFetchResponse(401, { error: 'unauthorized' });
    fetchMock.mockRejectedValueOnce(new Error('network'));

    const { getToken } = await import('@/api/auth');
    vi.mocked(getToken).mockReturnValue('fake-token');

    const { request } = await import('@/api/http');
    await expect(request('/groups', { method: 'GET' })).rejects.toThrow('Unauthorized');
    expect(mockLocation.href).toBe('/login');
  });
});

describe('getSetupToken', () => {
  it('stores a query token and removes it from the URL', async () => {
    mockLocation.href = '';
    Object.defineProperty(window, 'location', {
      value: { ...mockLocation, pathname: '/setup', search: '?setup_token=test-token&foo=bar', hash: '' },
      writable: true,
    });

    const { getSetupToken } = await import('@/api/http');
    expect(getSetupToken()).toBe('test-token');
    expect(replaceStateMock).toHaveBeenCalledWith(null, '', '/setup?foo=bar');
  });
});
