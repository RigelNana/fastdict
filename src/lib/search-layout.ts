// Use the visual viewport so the same input stays above a phone keyboard.
export function searchOffsets(height: number, top: number, box: number) {
  const available = Math.max(0, height - box)
  return { dock: Math.max(0, top + available), home: Math.max(0, top + available * .4) }
}
