import { useQuery } from '@tanstack/react-query'
import type { Bootstrap, Connection } from './types'
import { uploadChunks, type TransferProgress } from './chunk-upload'

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

export type UploadProgress = TransferProgress

export function apiUpload<T>(path: string, body: FormData, onProgress: (progress: UploadProgress) => void): Promise<T> {
  const files = body.getAll('files').filter((entry): entry is File => entry instanceof File)
  const resourceTarget = path.match(/^\/dictionaries\/([^/]+)\/resources$/)?.[1]
  const chunked = (path === '/imports' || resourceTarget) && files.reduce((sum, file) => sum + file.size, 0) > 16 * 1024 ** 2
  if (chunked) return uploadChunks<T>(files, {
    create: files => api('/uploads', { method: 'POST', body: JSON.stringify({
      kind: resourceTarget ? 'resources' : 'import', dictionary_id: resourceTarget ?? null, files,
    }) }),
    write: (id, index, offset, bytes, progress) => xhrUpload(`/uploads/${encodeURIComponent(id)}/files/${index}?offset=${offset}`, bytes, state => progress(state.loaded), 'PUT'),
    complete: id => api<T>(`/uploads/${encodeURIComponent(id)}/complete`, { method: 'POST' }),
    discard: id => api(`/uploads/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  }, onProgress)
  return xhrUpload(path, body, onProgress)
}

function xhrUpload<T>(path: string, body: FormData | Blob, onProgress: (progress: UploadProgress) => void, method = 'POST'): Promise<T> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    const { token } = getConnection()
    xhr.open(method, apiUrl(path))
    if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`)
    if (body instanceof Blob) {
      xhr.setRequestHeader('Content-Type', 'application/octet-stream')
      xhr.timeout = 300_000
    }
    let progress: UploadProgress = { phase: 'uploading', loaded: 0, total: 0 }
    onProgress(progress)
    xhr.upload.onprogress = (event) => {
      progress = { phase: 'uploading', loaded: event.loaded, total: event.lengthComputable ? event.total : 0 }
      onProgress(progress)
    }
    xhr.upload.onload = () => onProgress({ ...progress, phase: 'processing' })
    xhr.onerror = () => reject(new ApiError('上传连接中断，请重试。', 0, 'unavailable'))
    xhr.ontimeout = () => reject(new ApiError('上传连接超时，请重试。', 0, 'unavailable'))
    xhr.onabort = () => reject(new DOMException('上传已取消', 'AbortError'))
    xhr.onload = () => {
      let data: { error?: { message?: string; code?: string } } | undefined
      try { data = JSON.parse(xhr.responseText) } catch { /* A proxy may return HTML. */ }
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new ApiError(data?.error?.message ?? (xhr.status === 413 ? '文件超过服务允许的上传大小。' : `上传失败（${xhr.status}）。`), xhr.status, data?.error?.code ?? 'upload_failed'))
      } else if (data === undefined) {
        reject(new ApiError('服务返回了无效的上传结果。', xhr.status, 'invalid_response'))
      } else resolve(data as T)
    }
    xhr.send(body)
  })
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
