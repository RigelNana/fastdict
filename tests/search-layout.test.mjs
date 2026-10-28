import test from 'node:test'
import assert from 'node:assert/strict'
import { searchOffsets } from '../src/lib/search-layout.ts'

test('keyboard viewport resizing keeps the complete search field above the keyboard', () => {
  for (const [height, top, searchHeight] of [[844, 0, 84], [380, 0, 84], [380, 140, 118]]) {
    const { dock, home } = searchOffsets(height, top, searchHeight)
    assert.equal(dock + searchHeight, top + height)
    assert.ok(home >= top && home < dock)
  }
})

test('the home rests at the same relative position without moving above a tiny viewport', () => {
  assert.equal(searchOffsets(844, 0, 84).home, 304)
  assert.deepEqual(searchOffsets(60, 20, 84), { dock: 20, home: 20 })
})
