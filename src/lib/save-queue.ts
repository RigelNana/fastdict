// Writes to the same resource stay ordered, including across editor mounts.
const writes = new Map<string, Promise<unknown>>()

export function enqueueSave<T>(key: string, write: () => Promise<T>): Promise<T> {
  const previous = writes.get(key) ?? Promise.resolve()
  const result = previous.catch(() => undefined).then(write)
  writes.set(key, result)
  void result.finally(() => {
    if (writes.get(key) === result) writes.delete(key)
  }).catch(() => undefined)
  return result
}
