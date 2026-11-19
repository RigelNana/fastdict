// Use the visual viewport so the same input stays above a phone keyboard.
// Round to whole CSS pixels so GPU-translated text never lands on a subpixel offset.
export function searchOffsets(height: number, top: number, box: number) {
  const available = Math.max(0, height - box)
  return {
    dock: Math.round(Math.max(0, top + available)),
    home: Math.round(Math.max(0, top + available * 0.4)),
  }
}
