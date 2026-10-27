import type { Suggestion, SuggestionResponse } from './types'

export const normalizeHeadword = (value: string) => value.normalize('NFC').toLowerCase().trim()

// Only carry forward real matches from the same search scope while the new request runs.
// Placeholder data stays outside the query cache, so a partial list cannot become authoritative.
export function matchingHeadwords(
  previous: SuggestionResponse | undefined,
  previousKey: readonly unknown[] | undefined,
  currentKey: readonly unknown[],
): SuggestionResponse | undefined {
  if (!previous || previousKey?.[0] !== 'suggest' || currentKey[0] !== 'suggest' ||
    previousKey[2] !== currentKey[2] || previousKey[3] !== 'headword' || currentKey[3] !== 'headword') return
  const query = normalizeHeadword(String(currentKey[1]))
  if (!query) return
  const items = previous.items.flatMap<Suggestion>((item) => {
    const word = normalizeHeadword(item.word)
    return word.startsWith(query)
      ? [{ ...item, match_kind: word === query ? 'exact' : 'prefix' }]
      : []
  })
  if (!items.length) return
  return { ...previous, items, truncated: true }
}
