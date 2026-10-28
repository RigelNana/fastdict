export interface WordVisit {
  key: string
  word: string
  url: string
}

export interface WordTrail {
  entries: WordVisit[]
  index: number
}

export function readWordTrail(value: string | null): WordTrail {
  try {
    const trail = JSON.parse(value ?? 'null') as WordTrail
    if (trail && Array.isArray(trail.entries) && trail.entries.length <= 100 &&
      Number.isInteger(trail.index) && trail.index >= 0 && trail.index < trail.entries.length &&
      trail.entries.every(visit => visit && typeof visit.key === 'string' &&
        typeof visit.word === 'string' && typeof visit.url === 'string' &&
        /^\/word\/[^/?#]+(?:\?[^#]*)?$/.test(visit.url))) return trail
  } catch { /* Ignore obsolete session state. */ }
  return { entries: [], index: -1 }
}

export function visitWord(trail: WordTrail, visit: WordVisit): WordTrail {
  const existing = trail.entries.findIndex(entry => entry.key === visit.key)
  if (existing >= 0) {
    if (existing === trail.index && trail.entries[existing].url === visit.url) return trail
    const entries = [...trail.entries]
    entries[existing] = visit
    return { entries, index: existing }
  }
  // Group switches and returning from settings stay on the same word.
  if (trail.entries[trail.index]?.word === visit.word) {
    const entries = [...trail.entries]
    entries[trail.index] = visit
    return { entries, index: trail.index }
  }
  const entries = [...trail.entries.slice(0, trail.index + 1), visit].slice(-100)
  return { entries, index: entries.length - 1 }
}
