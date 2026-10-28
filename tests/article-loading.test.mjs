import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { articleDocument } from '../src/lib/article-storage.ts'

const bridge = readFileSync(new URL('../server/src/render.rs', import.meta.url), 'utf8').match(/const BRIDGE: &str = r#"([\s\S]*?)"#;/)[1]
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r }); return { promise, resolve } }
function reader({ theme = 'light', links = [], fonts = Promise.resolve() } = {}) {
  const listeners = new Map(), messages = [], frames = [], scripts = [], timers = new Map()
  const root = { getBoundingClientRect: () => ({ height: 640 }), scrollHeight: 640 }
  const seed = { textContent: JSON.stringify({ theme, themeRuntimeUrl: 'https://app.example/darkreader.js', font_size: 20, line_height: 1.6, reading_mode: true }), remove() {} }
  const document = {
    readyState: 'loading', documentElement: { dataset: { fastdictTheme: theme } },
    body: { dataset: {}, style: { removeProperty() {}, setProperty() {} } }, fonts: { ready: fonts },
    currentScript: { nonce: 'trusted' }, getElementById: id => id === 'fastdict-content' ? root : seed,
    addEventListener: (name, fn) => listeners.set(name, fn), querySelectorAll: () => links,
    createElement: () => ({}), head: { append: script => scripts.push(script) },
  }
  const parent = { postMessage: m => messages.push(m) }
  let timerId = 0
  const context = vm.createContext({ document, parent, addEventListener: (name, fn) => listeners.set(name, fn),
    ResizeObserver: class { observe() {} }, requestAnimationFrame: fn => frames.push(fn),
    setTimeout: (fn, ms) => { timers.set(++timerId, { fn, ms }); return timerId }, clearTimeout: id => timers.delete(id),
    URL, Response, ArrayBuffer, scrollY: 0,
  })
  vm.runInContext(bridge, context)
  const settle = async () => { for (let i = 0; i < 8; i++) { await Promise.resolve(); while (frames.length) frames.shift()() } }
  return { context, document, listeners, messages, scripts, timers, settle, parent }
}

test('initial appearance, stylesheet and font layout settle before one ready message; images do not gate text', async () => {
  const events = new Map(), font = deferred()
  const link = { sheet: null, addEventListener: (name, fn) => events.set(name, fn), removeEventListener: name => events.delete(name) }
  const r = reader({ links: [link], fonts: font.promise })
  assert.equal(r.document.body.style.fontSize, '20px')
  assert.equal(r.document.body.style.lineHeight, '1.6')
  r.listeners.get('DOMContentLoaded')()
  await r.settle()
  assert.equal(r.messages.some(m => m.type === 'fastdict:ready'), false)
  events.get('load')()
  await r.settle()
  assert.equal(r.messages.some(m => m.type === 'fastdict:ready'), false)
  font.resolve()
  await r.settle()
  assert.equal(r.messages.filter(m => m.type === 'fastdict:ready').length, 1)
  assert.equal(r.messages.find(m => m.type === 'fastdict:ready').height, 640)
  // No window/iframe load event was fired; an image can still be downloading.
  r.listeners.get('load')()
  await r.settle()
  assert.equal(r.messages.filter(m => m.type === 'fastdict:ready').length, 1)
})

test('dark adaptation starts before iframe load and waits for CSS analysis, not image analysis', async () => {
  const r = reader({ theme: 'dark' })
  assert.equal(r.scripts.length, 1)
  assert.equal(r.scripts[0].nonce, 'trusted')
  let fetchCSS, enabled = false
  r.context.DarkReader = {
    isEnabled: () => enabled, setFetchMethod: fn => { fetchCSS = fn },
    enable() {
      enabled = true
      void fetchCSS('https://app.example/style.css').catch(() => {})
      void r.context.DarkReader.Plugins.fetch({ url: 'https://app.example/slow-image.png', responseType: 'data-url' }).catch(() => {})
    },
  }
  r.scripts[0].onload()
  r.listeners.get('DOMContentLoaded')()
  await r.settle()
  assert.equal(r.messages.some(m => m.type === 'fastdict:ready'), false)
  const css = r.messages.find(m => m.type === 'fastdict:theme-resource' && m.url.endsWith('.css'))
  r.listeners.get('message')({ source: r.parent, data: { type: 'fastdict:theme-resource-result', requestId: css.requestId, body: new ArrayBuffer(0), contentType: 'text/css' } })
  await r.settle()
  assert.equal(r.messages.filter(m => m.type === 'fastdict:ready').length, 1)
  assert.ok(r.messages.some(m => m.type === 'fastdict:theme-resource' && m.url.endsWith('.png')))
})

test('the embedded appearance cannot inject scripts or HTML', () => {
  const html = '<html ><head><script type="application/json" id="fastdict-initial-style">{}</script></head><body>Text</body></html>'
  const family = '</script><script>bad()</script>$&'
  const result = articleDocument(html, 'dark', { local: {}, session: {} }, { font_family: family, font_size: 22 })
  assert.equal(result.match(/<script/g).length, 1)
  const seed = JSON.parse(result.match(/id="fastdict-initial-style">(.*?)<\/script>/)[1])
  assert.equal(seed.font_family, family)
  assert.equal(seed.font_size, 22)
  assert.equal(seed.theme, 'dark')
})

test('hidden-frame animation throttling cannot leave a ready article behind its placeholder', async () => {
  const r = reader()
  r.listeners.get('DOMContentLoaded')()
  for (let i = 0; i < 8; i++) await Promise.resolve()
  // Never execute the frame queue, as a browser may throttle an invisible iframe.
  for (let i = 0; i < 4; i++) {
    for (const [id, timer] of r.timers) {
      if (timer.ms === 32) { r.timers.delete(id); timer.fn() }
    }
    await Promise.resolve()
  }
  assert.equal(r.messages.filter(m => m.type === 'fastdict:ready').length, 1)
})

test('a stylesheet that failed before DOM readiness does not add a new timeout', async () => {
  const link = { sheet: null, dataset: { fastdictLoaded: 'error' } }
  const r = reader({ links: [link] })
  r.listeners.get('DOMContentLoaded')()
  await r.settle()
  assert.equal(r.messages.filter(m => m.type === 'fastdict:ready').length, 1)
  assert.equal([...r.timers.values()].some(timer => timer.ms === 5000), false)
})
