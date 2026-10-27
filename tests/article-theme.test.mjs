import test from 'node:test'
import assert from 'node:assert/strict'
import { articleThemeResource, fetchArticleThemeResource } from '../src/lib/article-theme.ts'

const base = 'http://127.0.0.1:8765/api/v1/content/d/r/token/'
const html = `<!doctype html><html data-fastdict-assets="${base}"><head></head><body></body></html>`

test('theme resources stay within the current article capability', () => {
  assert.equal(articleThemeResource(html, base + 'styles/main.css'), base + 'styles/main.css')
  assert.equal(articleThemeResource(html, base + 'image.png?v=2'), base + 'image.png?v=2')
  for (const url of [
    'https://example.com/main.css', 'http://127.0.0.1:8765/api/v1/bootstrap',
    base + '../other/main.css', base + '%2e%2e/main.css', base.replace('token/', 'token-other/') + 'main.css',
    base.replace('127.0.0.1', 'user@127.0.0.1') + 'main.css', 'javascript:alert(1)', 'data:text/css,p{}',
  ]) assert.equal(articleThemeResource(html, url), null, url)
  assert.equal(articleThemeResource('<html><p>unscoped</p></html>', base + 'main.css'), null)
})

test('theme fetches omit credentials, disallow redirects, and return only CSS or image bytes', async (t) => {
  let options
  t.mock.method(globalThis, 'fetch', async (_url, init) => {
    options = init
    return new Response('p {color:red}', { headers: { 'content-type': 'text/css; charset=utf-8' } })
  })
  const result = await fetchArticleThemeResource(base + 'main.css')
  assert.equal(new TextDecoder().decode(result.body), 'p {color:red}')
  assert.equal(options.credentials, 'omit')
  assert.equal(options.redirect, 'error')
  assert.equal(options.referrerPolicy, 'no-referrer')
})

test('HTML and oversized theme responses are rejected, including unbounded streams', async (t) => {
  const fetch = t.mock.method(globalThis, 'fetch', async () => new Response('<html></html>', { headers: { 'content-type': 'text/html' } }))
  await assert.rejects(fetchArticleThemeResource(base + 'page.html'), /Unsupported/)
  fetch.mock.mockImplementation(async () => new Response('', { headers: { 'content-type': 'text/css', 'content-length': String(4 * 1024 * 1024 + 1) } }))
  await assert.rejects(fetchArticleThemeResource(base + 'huge.css'), /Unsupported/)
  let cancelled = false
  fetch.mock.mockImplementation(async () => new Response(new ReadableStream({
    start(controller) { controller.enqueue(new Uint8Array(4 * 1024 * 1024 + 1)) },
    cancel() { cancelled = true },
  }), { headers: { 'content-type': 'text/css' } }))
  await assert.rejects(fetchArticleThemeResource(base + 'stream.css'), /too large/)
  assert.equal(cancelled, true)
})
