import { useQuery } from '@tanstack/react-query'
import type { Bootstrap, Connection } from './types'

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public code: string,
  ) {
    super(message)
  }
}

const connectionKey = 'fastdict.connection'
export function getConnection(): Connection {
  try {
    const value = JSON.parse(sessionStorage.getItem(connectionKey) ?? '{}') as Partial<Connection>
    return {
      base_url:
        typeof value.base_url === 'string' ? value.base_url : (import.meta.env.VITE_API_URL ?? ''),
      token: typeof value.token === 'string' ? value.token : '',
    }
  } catch {
    return { base_url: import.meta.env.VITE_API_URL ?? '', token: '' }
  }
}

export function setConnection(connection: Connection) {
  const base_url = connection.base_url.trim().replace(/\/+$/, '')
  if (base_url) {
    const url = new URL(base_url)
    if (
      !['http:', 'https:'].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.pathname !== '/'
    ) {
      throw new Error('服务地址需要是 http(s) 站点根地址，不包含账号、路径或查询参数。')
    }
    if (location.protocol === 'https:' && url.protocol !== 'https:') {
      throw new Error('HTTPS 页面不能连接不安全的 HTTP 服务。')
    }
  }
  sessionStorage.setItem(
    connectionKey,
    JSON.stringify({ base_url, token: connection.token.trim() }),
  )
  window.dispatchEvent(new Event('fastdict:connection'))
}

export function apiUrl(path: string) {
  return `${getConnection().base_url}/api/v1${path.startsWith('/') ? path : `/${path}`}`
}

async function request(path: string, init: RequestInit = {}) {
  const headers = new Headers(init.headers)
  const { token } = getConnection()
  if (token) headers.set('Authorization', `Bearer ${token}`)
  if (init.body && !(init.body instanceof FormData) && !headers.has('Content-Type'))
    headers.set('Content-Type', 'application/json')
  let response: Response
  try {
    response = await fetch(apiUrl(path), { ...init, headers, credentials: 'omit' })
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') throw error
    throw new ApiError('无法连接词典服务。检查服务地址、网络和服务是否启动。', 0, 'unavailable')
  }
  if (!response.ok) {
    let message =
      response.status === 401
        ? '访问凭证无效，请检查服务连接。'
        : `服务请求失败（${response.status}）。`
    let code = 'request_failed'
    try {
      const body = (await response.json()) as { error?: { message?: string; code?: string } }
      message = body.error?.message ?? message
      code = body.error?.code ?? code
    } catch {
      /* Preserve HTTP error when a proxy returned non-JSON. */
    }
    throw new ApiError(message, response.status, code)
  }
  return response
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await request(path, init)
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export async function apiBlob(path: string, init?: RequestInit): Promise<Blob> {
  return (await request(path, init)).blob()
}

export function useBootstrap() {
  return useQuery({
    queryKey: ['bootstrap'],
    queryFn: ({ signal }) => api<Bootstrap>('/bootstrap', { signal }),
    staleTime: 10_000,
    refetchInterval: (query) =>
      query.state.data?.stats.jobs.some((job) => job.state === 'running' || job.state === 'queued')
        ? 1500
        : false,
  })
}
