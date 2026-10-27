// Keep connection and navigation state when an existing tab adopts the new name.
export function migrateSessionStorage(storage: Storage) {
  const keys = Array.from({ length: storage.length }, (_, index) => storage.key(index))
  for (const key of keys) {
    if (!key?.startsWith('mydict.')) continue
    const next = `fastdict.${key.slice('mydict.'.length)}`
    const value = storage.getItem(key)
    if (value !== null && storage.getItem(next) === null) storage.setItem(next, value)
    storage.removeItem(key)
  }
}
