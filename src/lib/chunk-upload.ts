export interface TransferProgress {
  phase: 'uploading' | 'processing'
  loaded: number
  total: number
  retry?: number
}
export interface TransferFile {
  name: string
  size: number
  slice(start: number, end: number): Blob
}
export interface ChunkTransport<T> {
  create(files: { path: string; size: number }[]): Promise<{ id: string; chunk_size: number }>
  write(id: string, index: number, offset: number, bytes: Blob, progress: (loaded: number) => void): Promise<{ received: number }>
  complete(id: string): Promise<T>
  discard(id: string): Promise<unknown>
}
const maxBytes = 2 * 1024 ** 3
const pause = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))
function transient(error: unknown) {
  const status = (error as { status?: number } | null)?.status
  return status === 0 || status === 408 || status === 429 || (typeof status === 'number' && status >= 500)
}

export async function uploadChunks<T>(files: TransferFile[], transport: ChunkTransport<T>, progress: (state: TransferProgress) => void, wait = pause): Promise<T> {
  const total = files.reduce((sum, file) => sum + file.size, 0)
  if (!files.length || !Number.isSafeInteger(total) || files.some(file => !Number.isSafeInteger(file.size) || file.size < 0)) throw new Error('上传文件无效。')
  if (total > maxBytes) throw new Error('本次上传总大小不能超过 2 GiB。')
  let loaded = 0
  progress({ phase: 'uploading', loaded, total })
  const session = await transport.create(files.map(file => ({ path: file.name, size: file.size })))
  const retry = async <R>(operation: () => Promise<R>, phase: TransferProgress['phase']): Promise<R> => {
    for (let attempt = 0; ; attempt++) {
      try { return await operation() }
      catch (error) {
        if (attempt >= 2 || !transient(error)) throw error
        progress({ phase, loaded, total, retry: attempt + 1 })
        await wait(400 * 2 ** attempt)
      }
    }
  }
  try {
    if (!Number.isSafeInteger(session.chunk_size) || session.chunk_size <= 0 || session.chunk_size > 8 * 1024 ** 2) throw new Error('服务返回的上传块大小无效。')
    let completed = 0
    for (const [index, file] of files.entries()) {
      for (let offset = 0; offset < file.size; offset += session.chunk_size) {
        const end = Math.min(file.size, offset + session.chunk_size)
        const chunk = file.slice(offset, end)
        const result = await retry(() => transport.write(session.id, index, offset, chunk, sent => {
          loaded = Math.max(loaded, completed + offset + Math.min(chunk.size, Math.max(0, sent)))
          progress({ phase: 'uploading', loaded, total })
        }), 'uploading')
        if (result.received < end || result.received > file.size) throw new Error('服务未完整接收当前上传块，请重试。')
        loaded = Math.max(loaded, completed + end)
        progress({ phase: 'uploading', loaded, total })
      }
      completed += file.size
    }
    loaded = total
    progress({ phase: 'processing', loaded, total })
    return await retry(() => transport.complete(session.id), 'processing')
  } finally {
    // Deletes only transport staging. Published resources/import previews stay intact.
    void transport.discard(session.id).catch(() => {})
  }
}
