import test from 'node:test'
import assert from 'node:assert/strict'
import { readBootstrapCache, writeBootstrapCache, readCachedTheme } from '../src/lib/bootstrap-cache.ts'
import { iconCandidates } from '../src/lib/dictionary-icon.ts'

function memoryStorage() {
  const map = new Map()
  return {
    get length() { return map.size },
    key: (index) => [...map.keys()][index] ?? null,
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => map.set(key, String(value)),
    removeItem: (key) => map.delete(key),
  }
}

test('bootstrap cache scopes by origin, strips transient jobs, and persists theme preference', () => {
  const storage = memoryStorage()
  const sample = {
    dictionaries: [],
    groups: [{ id: 'en', name: 'English', dictionary_ids: [], sort: [] }],
    history: [{ id: '1', word: 'apple', group_id: 'all', at: '2026-10-29T00:00:00Z' }],
    settings: {
      application: { default_group: 'en', history_enabled: true, history_limit: 200 },
      appearance: { theme: 'dark', font_size: 18, line_height: 1.8, font_family: '', content_width: 840, motion: true, reading_mode: false },
      spelling: { enabled: true, distance: 1 },
      security: { external_links: 'ask' },
      performance: { max_open_readers: 8, cache_mb: 128, query_workers: 4 },
    },
    stats: {
      words: 1986054,
      open_readers: 6,
      cache_bytes: 1024,
      jobs: [{ id: 'j1', kind: 'import', dictionary_id: null, state: 'running', progress: 1, total: 2, error: null }],
    },
  }
  writeBootstrapCache('http://127.0.0.1:8765/', sample, storage)
  assert.equal(readCachedTheme(storage), 'dark')
  const restored = readBootstrapCache('http://127.0.0.1:8765', storage)
  assert.ok(restored)
  assert.equal(restored.groups[0].name, 'English')
  assert.deepEqual(restored.stats.jobs, [])
  assert.equal(readBootstrapCache('http://other.example:8765', storage), undefined)
})

test('dictionary icon discovery finds stem-named companion images and skips multi-gigabyte MDD scans', () => {
  const ahd = iconCandidates({
    name: 'AHD双解',
    icon: null,
    resources: [
      { id: 'm1', path: 'AHD双解.mdd', kind: 'mdd', size: 780 * 1024 * 1024, enabled: true, order: 0, key: '' },
      { id: 'i1', path: 'AHD双解.jpg', kind: 'image', size: 11 * 1024, enabled: true, order: 1, key: 'AHD双解.jpg' },
    ],
  })
  assert.equal(ahd.length, 1)
  assert.equal(ahd[0].resource.id, 'i1')
  assert.equal(ahd[0].path, '')

  const ldoce = iconCandidates({
    name: 'LDOCE6EC',
    icon: null,
    resources: [
      { id: 'm2', path: 'LDOCE6EC.mdd', kind: 'mdd', size: 1300 * 1024 * 1024, enabled: true, order: 0, key: '' },
    ],
  })
  assert.deepEqual(ldoce, [], '1.3 GB media archives are not auto-probed when no icon is configured')

  const explicit = iconCandidates({
    name: 'LDOCE6EC',
    icon: 'custom/logo.png',
    resources: [
      { id: 'm2', path: 'LDOCE6EC.mdd', kind: 'mdd', size: 1300 * 1024 * 1024, enabled: true, order: 0, key: '' },
    ],
  })
  assert.equal(explicit.length, 1)
  assert.equal(explicit[0].path, 'custom/logo.png')
})
