import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { enqueueSave } from '../src/lib/save-queue.ts'
import { scrollMetrics } from '../src/lib/scroll-metrics.ts'
import { moveResourceWithinKind } from '../src/lib/resource-order.ts'
import { migrateSessionStorage } from '../src/lib/migrate-session.ts'

test('renaming preserves session connection and navigation without replacing newer preferences', () => {
  const values = new Map([
    ['mydict.connection', '{"base_url":"http://localhost:8765"}'],
    ['mydict.sidebar-collapsed', 'true'], ['fastdict.sidebar-collapsed', 'false'],
    ['unrelated', 'keep'],
  ])
  const storage = {
    get length() { return values.size }, key: index => [...values.keys()][index],
    getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value),
    removeItem: key => values.delete(key),
  }
  migrateSessionStorage(storage)
  migrateSessionStorage(storage)
  assert.equal(values.get('fastdict.connection'), '{"base_url":"http://localhost:8765"}')
  assert.equal(values.get('fastdict.sidebar-collapsed'), 'false')
  assert.equal(values.get('unrelated'), 'keep')
  assert.equal([...values.keys()].some(key => key.startsWith('mydict.')), false)
})

const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r }); return { promise, resolve } }

test('custom scrollbar stays within its track at both ends and handles short pages', () => {
  const top = scrollMetrics(600, 2400, 536, -20)
  const bottom = scrollMetrics(600, 2400, 536, 9000)
  assert.equal(top.position, 0)
  assert.equal(bottom.position + bottom.thumb, 536)
  assert.equal(scrollMetrics(600, 400, 536, 0).range, 0)
  assert.equal(scrollMetrics(600, 200000, 20, 0).thumb, 20)
})

test('resource ordering moves only matching file types and preserves other bindings', () => {
  const resources = ['css', 'mdd', 'css', 'font', 'css'].map((kind, order) => ({ id: String(order), kind, order, enabled: true, key: String(order) }))
  const moved = moveResourceWithinKind(resources, '0', 1)
  assert.deepEqual(moved.map(r => r.id), ['2', '1', '0', '3', '4'])
  assert.deepEqual(moved.map(r => r.order), [0, 1, 2, 3, 4])
  assert.deepEqual(resources.map(r => r.id), ['0', '1', '2', '3', '4'])
  assert.equal(moveResourceWithinKind(resources, '1', 1), resources)
  assert.equal(moveResourceWithinKind(resources, 'missing', -1), resources)
})

test('rapid saves and a remounted editor cannot finish out of order', async () => {
  const gate = deferred()
  const writes = []
  const first = enqueueSave('settings', async () => { await gate.promise; writes.push('old') })
  const second = enqueueSave('settings', async () => { writes.push('new') })
  await enqueueSave('dictionary', async () => { writes.push('independent') })
  assert.deepEqual(writes, ['independent'])
  gate.resolve()
  await Promise.all([first, second])
  assert.deepEqual(writes, ['independent', 'old', 'new'])
})

test('a rejected save does not prevent retrying the resource', async () => {
  await assert.rejects(enqueueSave('retry', async () => { throw new Error('offline') }), /offline/)
  assert.equal(await enqueueSave('retry', async () => 'saved'), 'saved')
})

test('isolated article clears selection only for its parent and reports pointer focus', () => {
  const source = readFileSync(new URL('../server/src/render.rs', import.meta.url), 'utf8')
  const bridge = source.match(/const BRIDGE: &str = r#"([\s\S]*?)"#;/)[1]
  const listeners = new Map()
  const messages = []
  let cleared = 0
  const parent = { postMessage: value => messages.push(value) }
  const on = (name, callback) => listeners.set(name, callback)
  const root = { getBoundingClientRect: () => ({ height: 120 }), scrollHeight: 120 }
  vm.runInNewContext(bridge, {
    parent, document: { getElementById: () => root, addEventListener: on },
    addEventListener: on, ResizeObserver: class { observe() {} },
    requestAnimationFrame: callback => callback(), getSelection: () => ({ removeAllRanges: () => cleared++ }),
    URL, scrollY: 0,
  })
  listeners.get('message')({ source: {}, data: { type: 'fastdict:clear-selection' } })
  assert.equal(cleared, 0)
  listeners.get('message')({ source: parent, data: { type: 'fastdict:clear-selection' } })
  assert.equal(cleared, 1)
  listeners.get('pointerdown')()
  assert.equal(messages.at(-1).type, 'fastdict:focus')
})

test('article theme switches through the trusted bridge without overriding source colors', () => {
  const source = readFileSync(new URL('../server/src/render.rs', import.meta.url), 'utf8')
  const bridge = source.match(/const BRIDGE: &str = r#"([\s\S]*?)"#;/)[1]
  const listeners = new Map()
  const parent = { postMessage() {} }
  const root = { getBoundingClientRect: () => ({ height: 120 }), scrollHeight: 120 }
  const body = { dataset: { fastdictTypography: 'original' }, style: { color: 'navy' } }
  const documentElement = { dataset: { fastdictTheme: 'light' } }
  let enabled = false
  let activations = 0
  let themeOptions
  const DarkReader = {
    isEnabled: () => enabled,
    enable(options) { enabled = true; activations++; themeOptions = options },
    disable() { enabled = false },
  }
  const on = (name, callback) => listeners.set(name, callback)
  vm.runInNewContext(bridge, {
    parent, DarkReader, document: { getElementById: () => root, addEventListener: on, body, documentElement },
    addEventListener: on, ResizeObserver: class { observe() {} }, requestAnimationFrame: callback => callback(), URL, scrollY: 0,
  })
  const send = (theme, source = parent) => listeners.get('message')({ source, data: { type: 'fastdict:style', theme } })
  send('dark', {})
  assert.equal(documentElement.dataset.fastdictTheme, 'light')
  send('dark')
  assert.equal(documentElement.dataset.fastdictTheme, 'dark')
  assert.equal(enabled, true)
  assert.equal(themeOptions.darkSchemeBackgroundColor, '#09090b')
  assert.equal(body.style.color, 'navy', 'the engine adapts source styles instead of overwriting their colors')
  send('dark')
  assert.equal(activations, 1, 'font and layout updates must not restart the theme engine')
  send('invalid')
  assert.equal(documentElement.dataset.fastdictTheme, 'dark')
  send('light')
  assert.equal(documentElement.dataset.fastdictTheme, 'light')
  assert.equal(enabled, false)
  assert.equal(body.style.color, 'navy')
})
