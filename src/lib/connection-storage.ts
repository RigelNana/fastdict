import type { Connection } from './types'

export const connectionKey = 'fastdict.connection'
type StorageArea = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

export function browserStorage(area: 'localStorage' | 'sessionStorage'): StorageArea | undefined {
  try { return window[area] } catch { return undefined }
}

function read(storage: StorageArea | undefined, fallbackBase: string): Connection | undefined {
  try {
    const value: unknown = JSON.parse(storage?.getItem(connectionKey) ?? 'null')
    if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
    const connection = value as Partial<Connection>
    return {
      base_url: typeof connection.base_url === 'string' ? connection.base_url : fallbackBase,
      token: typeof connection.token === 'string' ? connection.token : '',
    }
  } catch { return undefined }
}

function removeLegacy(session: StorageArea | undefined) {
  try { session?.removeItem(connectionKey) } catch { /* Local storage remains authoritative. */ }
}

export function readConnection(local: StorageArea | undefined, session: StorageArea | undefined, fallbackBase = ''): Connection {
  const saved = read(local, fallbackBase)
  if (saved) {
    removeLegacy(session)
    return saved
  }
  const legacy = read(session, fallbackBase)
  if (legacy) {
    try {
      if (local) {
        local.setItem(connectionKey, JSON.stringify(legacy))
        removeLegacy(session)
      }
    } catch { /* Keep the existing session usable when persistent storage is unavailable. */ }
    return legacy
  }
  return { base_url: fallbackBase, token: '' }
}

export function saveConnection(local: StorageArea | undefined, session: StorageArea | undefined, connection: Connection) {
  try {
    if (!local) throw new Error('Storage unavailable')
    local.setItem(connectionKey, JSON.stringify(connection))
  } catch {
    throw new Error('无法在此浏览器保存连接，请允许本站保存数据后重试。')
  }
  removeLegacy(session)
}
