import test from 'node:test'
import assert from 'node:assert/strict'
import { readWordTrail, visitWord } from '../src/lib/word-navigation.ts'

const visit = (word, key = word, group = 'all') => ({ word, key, url: `/word/${encodeURIComponent(word)}?group=${group}` })

test('previous word walks backward and a new search branches from that word', () => {
  let trail = readWordTrail(null)
  for (const word of ['下方', '上方', 'うしうま']) trail = visitWord(trail, visit(word))
  trail = visitWord(trail, trail.entries[trail.index - 1])
  assert.equal(trail.entries[trail.index].word, '上方')
  trail = visitWord(trail, trail.entries[trail.index - 1])
  assert.equal(trail.entries[trail.index].word, '下方')
  assert.equal(trail.index, 0)
  trail = visitWord(trail, visit('辞典'))
  assert.deepEqual(trail.entries.map(entry => entry.word), ['下方', '辞典'])
})

test('group changes and returning from settings do not add a previous word', () => {
  let trail = visitWord(readWordTrail(null), visit('下方'))
  trail = visitWord(trail, visit('下方', 'group-switch', 'japanese'))
  trail = visitWord(trail, visit('下方', 'from-settings', 'japanese'))
  assert.equal(trail.entries.length, 1)
  assert.equal(trail.index, 0)
  assert.equal(trail.entries[0].url, '/word/%E4%B8%8B%E6%96%B9?group=japanese')
  assert.deepEqual(readWordTrail(JSON.stringify(trail)), trail)
})

test('browser back/forward selects its visit and new sessions reject invalid targets', () => {
  let trail = visitWord(visitWord(readWordTrail(null), visit('下方')), visit('上方'))
  trail = visitWord(trail, visit('下方'))
  assert.equal(trail.index, 0)
  trail = visitWord(trail, visit('上方'))
  assert.equal(trail.index, 1)
  for (const value of ['bad json', JSON.stringify({ entries: [{ ...visit('下方'), url: 'https://example.com' }], index: 0 })]) {
    assert.deepEqual(readWordTrail(value), { entries: [], index: -1 })
  }
})
