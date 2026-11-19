import test from 'node:test'
import assert from 'node:assert/strict'
import { connectionKey, readConnection, saveConnection } from '../src/lib/connection-storage.ts'

function storage(initial = {}) {
  const values = new Map(Object.entries(initial))
  return {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: key => values.delete(key),
  }
}
const connection = { base_url: 'https://dict.example.com', token: 'test-access-token' }

test('a new browser session restores the locally saved address and token', () => {
  const local = storage()
  saveConnection(local, storage(), connection)
  assert.deepEqual(readConnection(local, storage()), connection)
})

test('the previous session is migrated without losing unrelated navigation state', () => {
  const local = storage()
  const session = storage({ [connectionKey]: JSON.stringify(connection), 'fastdict.group': 'language' })
  assert.deepEqual(readConnection(local, session), connection)
  assert.deepEqual(JSON.parse(local.getItem(connectionKey)), connection)
  assert.equal(session.getItem(connectionKey), null)
  assert.equal(session.getItem('fastdict.group'), 'language')
  assert.deepEqual(readConnection(local, storage()), connection)
})

test('a stale tab cannot overwrite newer saved credentials or restore a cleared token', () => {
  const local = storage()
  const stale = storage({ [connectionKey]: JSON.stringify(connection) })
  const cleared = { base_url: '', token: '' }
  saveConnection(local, storage(), cleared)
  assert.deepEqual(readConnection(local, stale), cleared)
  assert.equal(stale.getItem(connectionKey), null)
})

test('corrupt saved data falls back safely and valid legacy data can repair it', () => {
  for (const raw of ['invalid json', 'null', '[]', '"value"']) {
    const local = storage({ [connectionKey]: raw })
    assert.deepEqual(readConnection(local, storage(), 'https://api.example.com'), { base_url: 'https://api.example.com', token: '' })
    assert.deepEqual(readConnection(local, storage({ [connectionKey]: JSON.stringify(connection) })), connection)
  }
})

test('blocked persistence keeps the current session usable without falsely reporting a save', () => {
  const blocked = { getItem() { throw new Error('blocked') }, setItem() { throw new Error('blocked') }, removeItem() {} }
  const session = storage({ [connectionKey]: JSON.stringify(connection) })
  assert.deepEqual(readConnection(blocked, session), connection)
  assert.equal(session.getItem(connectionKey), JSON.stringify(connection))
  assert.throws(() => saveConnection(blocked, session, connection), /无法在此浏览器保存连接/)
  assert.throws(() => saveConnection(undefined, session, connection), /无法在此浏览器保存连接/)
  assert.deepEqual(readConnection(undefined, undefined), { base_url: '', token: '' })
})
