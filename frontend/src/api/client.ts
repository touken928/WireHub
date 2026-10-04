import createClient from 'openapi-fetch'
import type { components, paths } from './schema'

export const client = createClient<paths>({ baseUrl: '' })

let currentToken = ''
client.use({ onRequest({ request }) { if (currentToken) request.headers.set('Authorization', `Bearer ${currentToken}`); else request.headers.delete('Authorization'); return request } })

export type Forward = components['schemas']['Forward']
export type NewForward = components['schemas']['NewForward']

export type NetworkSettings = components['schemas']['NetworkSettings']
export type SetupStatus = components['schemas']['SetupStatus']
export type SetupRequest = components['schemas']['SetupRequest']
export type Configuration = components['schemas']['Configuration']
export type RuntimeStatus = components['schemas']['RuntimeStatus']
export type PolicyResult = components['schemas']['PolicyResult']
export type SettingsRequest = components['schemas']['SettingsRequest']

export class ApiError extends Error {
  constructor(message: string, readonly status: number, readonly code?: string, readonly persistedRevision?: number) { super(message); this.name = 'ApiError' }
}

function throwSetupError(status: number, payload: unknown): never {
  const detail = payload && typeof payload === 'object' && 'message' in payload
    ? String((payload as { message: unknown }).message)
    : typeof payload === 'string' ? payload.trim() : ''
  const structured = payload && typeof payload === 'object' ? payload as { code?: string; persisted_revision?: number } : undefined
  throw new ApiError(detail || `Request failed (${status})`, status, structured?.code, structured?.persisted_revision)
}

export const configApi = {
  async get(): Promise<Configuration> {
    const { data, error, response } = await client.GET('/api/config', { cache: 'no-store' })
    if (!response.ok || !data) throwSetupError(response.status, error)
    return data
  },
  async save(expected_revision: number, changes: { id: string; allowed: string[] }[]): Promise<PolicyResult> {
    const { data, error, response } = await client.PUT('/api/policy', { body: { expected_revision, changes: changes.map(({ id, allowed }) => ({ group_id: id, allowed_groups: allowed })) }, cache: 'no-store' })
    if (!response.ok || !data) throwSetupError(response.status, error)
    return data
  },
  async status(): Promise<RuntimeStatus> {
    const { data, error, response } = await client.GET('/api/status', { cache: 'no-store' })
    if (!response.ok || !data) throwSetupError(response.status, error)
    return data
  },
}

export const setupApi = {
  async get(): Promise<SetupStatus> {
    const { data, error, response } = await client.GET('/api/setup', { cache: 'no-store' })
    if (error || !response.ok) throwSetupError(response.status, error)
    return data as SetupStatus
  },
  async create(body: SetupRequest): Promise<NetworkSettings> {
    const { data, error, response } = await client.POST('/api/setup', { body, cache: 'no-store' })
    if (error || !response.ok) throwSetupError(response.status, error)
    return data as NetworkSettings
  },
  async update(body: SettingsRequest): Promise<NetworkSettings> {
    const { data, error, response } = await client.PUT('/api/settings', { body, cache: 'no-store' })
    if (error || !response.ok) throwSetupError(response.status, error)
    return data as NetworkSettings
  },
}

export const forwardsApi = {
  async list(): Promise<Forward[]> {
    const { data, error, response } = await client.GET('/api/forwards')
    if (!response.ok || !data) throwSetupError(response.status, error)
    return data
  },
  async create(body: NewForward): Promise<Forward> {
    const { data, error, response } = await client.POST('/api/forwards', { body })
    if (!response.ok || !data) throwSetupError(response.status, error)
    return data
  },
  async remove(id: string) {
    const { error, response } = await client.DELETE('/api/forwards/{id}', { params: { path: { id } } })
    if (!response.ok) throwSetupError(response.status, error)
  },
}
export function rememberToken(token: string) { currentToken = token }
