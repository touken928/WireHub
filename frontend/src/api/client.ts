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
export type SettingsRequest = components['schemas']['SettingsRequest']

async function throwSetupError(response: Response): Promise<never> {
  let payload: unknown
  const text = await response.clone().text().catch(() => '')
  try { payload = JSON.parse(text) } catch { payload = text }

  const detail = payload && typeof payload === 'object' && 'message' in payload
    ? String((payload as { message: unknown }).message)
    : typeof payload === 'string' ? payload.trim() : ''
  throw new Error(detail || `Request failed (${response.status})`)
}

export const setupApi = {
  async get(): Promise<SetupStatus> {
    const { data, response } = await client.GET('/api/setup', { cache: 'no-store' })
    if (!response.ok) await throwSetupError(response)
    return data as SetupStatus
  },
  async create(body: SetupRequest): Promise<NetworkSettings> {
    const { data, response } = await client.POST('/api/setup', { body, cache: 'no-store' })
    if (!response.ok) await throwSetupError(response)
    return data as NetworkSettings
  },
  async update(body: SettingsRequest): Promise<NetworkSettings> {
    const { data, response } = await client.PUT('/api/settings', { body, cache: 'no-store' })
    if (!response.ok) await throwSetupError(response)
    return data as NetworkSettings
  },
}

export const forwardsApi = {
  async list(): Promise<Forward[]> {
    const { data, response } = await client.GET('/api/forwards')
    if (!response.ok || !data) throw new Error(`Unable to load forwards (${response.status})`)
    return data
  },
  async create(body: NewForward): Promise<Forward> {
    const { data, response } = await client.POST('/api/forwards', { body })
    if (!response.ok || !data) throw new Error(`Operation failed (${response.status})`)
    return data
  },
  async remove(id: string) {
    const { response } = await client.DELETE('/api/forwards/{id}', { params: { path: { id } } })
    if (!response.ok) throw new Error(`Unable to delete forward (${response.status})`)
  },
}
export function rememberToken(token: string) { currentToken = token }
