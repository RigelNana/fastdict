import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { articleDocument, dictionaryStorageKey, readArticleStorage, updateArticleStorage } from '../src/lib/article-storage.ts'

const source = readFileSync(new URL('../server/src/render.rs', import.meta.url), 'utf8')
const compatibility = source.match(/const SCRIPT_STORAGE: &str = r#"([\s\S]*?)"#;/)[1]

test('sandboxed scripts get synchronous isolated storage before their initialization', () => {
  const messages = []
  const window = {}
  Object.defineProperty(window, 'localStorage', { configurable: true, get() { throw new Error('Opaque origin') } })
  const seed = { textContent: JSON.stringify({ local: { OALDPE_showTranslation: '1' }, session: {} }), remove() {} }
  vm.runInNewContext(compatibility, { window, document: { getElementById: () => seed }, parent: { postMessage: m => messages.push(m) }, DOMException })
  assert.equal(window.localStorage.length, 1)
  assert.equal(window.localStorage.getItem('OALDPE_showTranslation'), '1')
  window.localStorage.setItem('OALDPE_showTranslation', 0)
  assert.equal(window.localStorage.getItem('OALDPE_showTranslation'), '0')
  assert.equal(messages.at(-1).area, 'local')
  window.sessionStorage.selected = 'noun'
  assert.equal(window.sessionStorage.key(0), 'selected')
  assert.equal(window.localStorage.getItem('selected'), null)
  window.localStorage.setItem('__proto__', 'safe value')
  assert.equal(window.localStorage.getItem('__proto__'), 'safe value')
  assert.throws(() => window.localStorage.setItem('large', 'x'.repeat(65537)), /limit exceeded/)
  assert.equal(window.localStorage.getItem('large'), null)
  window.localStorage.clear()
  assert.equal(window.localStorage.length, 0)
  assert.equal(window.sessionStorage.selected, 'noun')
})

test('dictionary settings persist per dictionary and never read or clear app credentials', () => {
  const values = new Map([['fastdict.connection', '{"token":"private"}']])
  const storage = { getItem: k => values.get(k) ?? null, setItem: (k,v) => values.set(k,v) }
  const one = dictionaryStorageKey('https://dict.example', 'one')
  const two = dictionaryStorageKey('https://dict.example', 'two')
  assert.equal(readArticleStorage(one, 'local', storage)['fastdict.connection'], undefined)
  assert.ok(updateArticleStorage(one, { area: 'local', method: 'setItem', key: 'theme', value: 'dark' }, storage))
  assert.equal(readArticleStorage(one, 'local', storage).theme, 'dark')
  assert.equal(readArticleStorage(two, 'local', storage).theme, undefined)
  assert.equal(readArticleStorage(one, 'session', storage).theme, undefined)
  assert.equal(updateArticleStorage(one, { area: 'local', method: 'setItem', key: 'large', value: 'x'.repeat(65537) }, storage), false)
  assert.ok(updateArticleStorage(one, { area: 'local', method: 'clear' }, storage))
  assert.equal(values.get('fastdict.connection'), '{"token":"private"}')
  assert.deepEqual(Object.keys(readArticleStorage(one, 'local', storage)), [])
})

test('storage values cannot inject markup or replacement directives into srcdoc', () => {
  const html = '<html ><head><script type="application/json" id="fastdict-script-storage">{}</script></head><body>Definition</body></html>'
  const value = '</script><script>alert(1)</script>$&'
  const document = articleDocument(html, 'dark', { local: { value }, session: {} })
  assert.match(document, /data-fastdict-theme="dark"/)
  assert.equal(document.match(/<script/g).length, 1)
  const seed = document.match(/id="fastdict-script-storage">(.*?)<\/script>/)[1]
  assert.equal(JSON.parse(seed).local.value, value)
})

test('dictionary link handlers run before the bridge and placeholder links do not navigate', () => {
  const bridge = source.match(/const BRIDGE: &str = r#"([\s\S]*?)"#;/)[1]
  const listeners = new Map(), messages = []
  const root = { getBoundingClientRect: () => ({ height: 120 }), scrollHeight: 120 }
  vm.runInNewContext(bridge, {
    parent: { postMessage: m => messages.push(m) }, document: { getElementById: () => root, addEventListener: (name, fn, capture) => listeners.set(name, { fn, capture }) },
    addEventListener() {}, ResizeObserver: class { observe() {} }, requestAnimationFrame: fn => fn(), URL, scrollY: 0,
  })
  const click = listeners.get('click')
  assert.notEqual(click.capture, true)
  messages.length = 0
  const event = href => ({ defaultPrevented: false, preventDefault() {}, target: { closest: () => ({ getAttribute: name => name === 'href' ? href : null }) } })
  click.fn(event('#'))
  click.fn(event('##'))
  click.fn({ ...event('entry://pear'), defaultPrevented: true })
  assert.equal(messages.length, 0)
  click.fn(event('entry://pear'))
  assert.equal(messages[0].type, 'fastdict:navigate')
  assert.equal(messages[0].word, 'pear')
})
