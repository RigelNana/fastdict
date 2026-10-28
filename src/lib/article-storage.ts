type Values = Record<string, string>
type Area = 'local' | 'session'
const limit = 256 * 1024
const memory = new Map<string, Values>()

export function dictionaryStorageKey(base: string, dictionary: string) {
  return `fastdict.dictionary-storage:${JSON.stringify([base, dictionary])}`
}

function validValues(value: unknown): value is Values {
  return !!value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length <= 512 && Object.entries(value).every(([key, item]) =>
      key.length <= 1024 && typeof item === 'string' && item.length <= 64 * 1024) &&
    JSON.stringify(value).length <= limit
}

function browserStorage(area: Area): Storage | undefined {
  try { return area === 'local' ? window.localStorage : window.sessionStorage } catch { return undefined }
}

export function readArticleStorage(scope: string, area: Area, storage = browserStorage(area)): Values {
  const key = `${scope}:${area}`
  try {
    const value: unknown = JSON.parse(storage?.getItem(key) ?? 'null')
    if (validValues(value)) { memory.set(key, value); return value }
  } catch { /* Restricted browser storage falls back to this tab's memory. */ }
  return memory.get(key) ?? Object.create(null) as Values
}

export function updateArticleStorage(scope: string, message: Record<string, unknown>, storage?: Storage) {
  const area = message.area
  if (area !== 'local' && area !== 'session') return false
  const next: Values = Object.assign(Object.create(null), readArticleStorage(scope, area, storage))
  if (message.method === 'clear') {
    for (const key of Object.keys(next)) delete next[key]
  } else {
    if (typeof message.key !== 'string' || message.key.length > 1024) return false
    if (message.method === 'removeItem') delete next[message.key]
    else if (message.method === 'setItem' && typeof message.value === 'string') next[message.key] = message.value
    else return false
  }
  if (!validValues(next)) return false
  const key = `${scope}:${area}`
  memory.set(key, next)
  try { (storage ?? browserStorage(area))?.setItem(key, JSON.stringify(next)) } catch { /* Keep the bounded in-memory settings. */ }
  return true
}

export function articleDocument(html: string, theme: 'light' | 'dark', storage: { local: Values; session: Values }, initialStyle: Record<string, unknown> = {}) {
  const seed = JSON.stringify(storage).replaceAll('<', '\\u003c')
  const style = JSON.stringify({ ...initialStyle, theme }).replaceAll('<', '\\u003c')
  return html.replace('<html ', `<html data-fastdict-theme="${theme}" `)
    .replace('<script type="application/json" id="fastdict-initial-style">{}</script>',
      () => `<script type="application/json" id="fastdict-initial-style">${style}</script>`)
    .replace('<script type="application/json" id="fastdict-script-storage">{}</script>',
      () => `<script type="application/json" id="fastdict-script-storage">${seed}</script>`)
}
