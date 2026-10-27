// The isolated reader may analyze only resources belonging to this article's capability.
export function articleThemeResource(html: string, requested: string): string | null {
  const base = html.match(/^<!doctype html><html\b[^>]*\bdata-fastdict-assets="([^"]+)"/i)?.[1]
  if (!base || requested.length > 8192) return null
  try {
    const root = new URL(base)
    const url = new URL(requested)
    const prefix = root.pathname.replace(/\/+$/, '') + '/'
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password ||
      url.origin !== root.origin || !url.pathname.startsWith(prefix)) return null
    return url.href
  } catch { return null }
}

export async function fetchArticleThemeResource(url: string) {
  const response = await fetch(url, { credentials: 'omit', redirect: 'error', referrerPolicy: 'no-referrer' })
  const contentType = response.headers.get('content-type') ?? ''
  const maxBytes = 4 * 1024 * 1024
  if (!response.ok || !/^(text\/css|image\/[^;\s]+)(;|$)/i.test(contentType) ||
    Number(response.headers.get('content-length')) > maxBytes) {
    await response.body?.cancel()
    throw new Error('Unsupported dictionary theme resource')
  }
  const reader = response.body?.getReader()
  if (!reader) throw new Error('Missing dictionary theme resource')
  const chunks: Uint8Array[] = []
  let length = 0
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      length += value.byteLength
      if (length > maxBytes) {
        await reader.cancel()
        throw new Error('Dictionary theme resource is too large')
      }
      chunks.push(value)
    }
  } finally { reader.releaseLock() }
  const bytes = new Uint8Array(length)
  let offset = 0
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength }
  return { body: bytes.buffer, contentType }
}
