import type { Bootstrap, Theme } from './types'

export const themePreferenceKey = 'fastdict.theme-preference'
const cachePrefix = 'fastdict.bootstrap-cache:'
const maxCacheBytes = 512 * 1024

function defaultLocalStorage(): Storage | undefined {
  try {
    return globalThis.localStorage
  } catch {
    return undefined
  }
}

function cacheKey(baseUrl: string) {
  return `${cachePrefix}${baseUrl.trim().replace(/\/+$/, '')}`
}

function isValidBootstrap(value: unknown): value is Bootstrap {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Partial<Bootstrap>
  return (
    Array.isArray(candidate.dictionaries) &&
    Array.isArray(candidate.groups) &&
    Array.isArray(candidate.history) &&
    Boolean(candidate.settings?.appearance) &&
    Boolean(candidate.settings?.application) &&
    Boolean(candidate.stats)
  )
}

export function readCachedTheme(storage = defaultLocalStorage()): Theme {
  const saved = storage?.getItem(themePreferenceKey)
  return saved === 'light' || saved === 'dark' || saved === 'system' ? saved : 'system'
}

export function writeCachedTheme(theme: Theme, storage = defaultLocalStorage()) {
  try {
    storage?.setItem(themePreferenceKey, theme)
  } catch {
    /* Ignore restricted storage. */
  }
}

export function readBootstrapCache(
  baseUrl: string,
  storage = defaultLocalStorage(),
): Bootstrap | undefined {
  try {
    const raw = storage?.getItem(cacheKey(baseUrl))
    if (!raw || raw.length > maxCacheBytes) return undefined
    const parsed: unknown = JSON.parse(raw)
    return isValidBootstrap(parsed) ? parsed : undefined
  } catch {
    return undefined
  }
}

export function writeBootstrapCache(
  baseUrl: string,
  data: Bootstrap,
  storage = defaultLocalStorage(),
) {
  writeCachedTheme(data.settings.appearance.theme, storage)
  try {
    // Active jobs are transient; avoid persisting running state across reloads.
    const snapshot: Bootstrap = {
      ...data,
      stats: { ...data.stats, jobs: [] },
    }
    const serialized = JSON.stringify(snapshot)
    if (serialized.length <= maxCacheBytes) {
      storage?.setItem(cacheKey(baseUrl), serialized)
      return
    }
    // Trim bulky raw metadata attributes/descriptions if a user has hundreds of dictionaries.
    const compact: Bootstrap = {
      ...snapshot,
      dictionaries: snapshot.dictionaries.map((dictionary) => ({
        ...dictionary,
        custom_css: '',
        metadata: { ...dictionary.metadata, description: '', attributes: {} },
      })),
    }
    const compactSerialized = JSON.stringify(compact)
    if (compactSerialized.length <= maxCacheBytes) {
      storage?.setItem(cacheKey(baseUrl), compactSerialized)
    }
  } catch {
    /* Ignore storage quota or privacy restrictions. */
  }
}
