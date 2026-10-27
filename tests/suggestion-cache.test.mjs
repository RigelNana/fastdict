import test from 'node:test'
import assert from 'node:assert/strict'
import { QueryClient, QueryObserver } from '@tanstack/react-query'
import { matchingHeadwords, normalizeHeadword } from '../src/lib/suggestion-cache.ts'

const key = (q, group = 'all', mode = 'headword') => ['suggest', q, group, mode]
const response = (...words) => ({
  items: words.map(word => ({ word, dictionary_ids: ['d'], match_kind: 'prefix', preview: null })),
  truncated: false, elapsed_ms: 1,
})
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r }); return { promise, resolve } }

test('typing narrows existing headwords and updates exact matches without altering cached data', () => {
  const cached = response('Apple', 'application', 'apricot')
  const narrowed = matchingHeadwords(cached, key('a'), key(' APPLE '))
  assert.deepEqual(narrowed.items.map(item => [item.word, item.match_kind]), [['Apple', 'exact']])
  assert.equal(narrowed.truncated, true)
  assert.equal(cached.items.length, 3)
  assert.equal(cached.items[0].match_kind, 'prefix')
  assert.equal(normalizeHeadword(' Cafe\u0301 '), 'café')
  assert.equal(matchingHeadwords(response('Café'), key('ca'), key('cafe\u0301')).items[0].match_kind, 'exact')
})

test('provisional candidates never cross groups or modes, and an incomplete miss is not cached as empty', () => {
  const cached = response('apple')
  assert.equal(matchingHeadwords(cached, key('a'), key('a', 'another')), undefined)
  assert.equal(matchingHeadwords(cached, key('a'), key('a', 'all', 'fulltext')), undefined)
  assert.equal(matchingHeadwords(cached, key('a', 'all', 'fulltext'), key('a')), undefined)
  assert.equal(matchingHeadwords(cached, key('a'), key('banana')), undefined)
  assert.equal(matchingHeadwords(cached, key('a'), key('')), undefined)
  assert.equal(matchingHeadwords(undefined, undefined, key('a')), undefined)
})

test('rapid typing cancels obsolete requests, preserves valid candidates, and restores cached queries immediately', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 30000 } } })
  const initial = response('apple', 'application', 'apricot')
  client.setQueryData(key('a'), initial)
  let calls = 0
  let cancelled = false
  const old = deferred()
  const latest = deferred()
  const options = (q, queryFn) => ({
    queryKey: key(q), queryFn,
    placeholderData: (data, previous) => matchingHeadwords(data, previous?.queryKey, key(q)),
  })
  const observer = new QueryObserver(client, options('a', async () => { calls++; return initial }))
  const unsubscribe = observer.subscribe(() => {})
  try {
    observer.setOptions(options('ap', ({ signal }) => {
      calls++
      signal.addEventListener('abort', () => { cancelled = true })
      return old.promise
    }))
    assert.equal(calls, 1, 'starts the new request without waiting for a typing debounce')
    assert.equal(observer.getCurrentResult().isPlaceholderData, true)
    assert.equal(observer.getCurrentResult().data.items.length, 3)
    assert.equal(client.getQueryData(key('ap')), undefined, 'partial candidates do not populate the cache')

    observer.setOptions(options('apple', () => { calls++; return latest.promise }))
    assert.equal(cancelled, true)
    assert.deepEqual(observer.getCurrentResult().data.items.map(item => item.word), ['apple'])
    const finished = observer.refetch()
    latest.resolve(response('apple', 'apple tree'))
    await finished
    old.resolve(response('apricot'))
    await old.promise
    assert.deepEqual(observer.getCurrentResult().data.items.map(item => item.word), ['apple', 'apple tree'])

    observer.setOptions(options('a', async () => { calls++; return initial }))
    assert.equal(observer.getCurrentResult().isFetching, false)
    assert.equal(observer.getCurrentResult().data, initial)
    assert.equal(calls, 2)
  } finally {
    unsubscribe()
    client.clear()
  }
})
