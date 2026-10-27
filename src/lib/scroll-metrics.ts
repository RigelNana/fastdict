export function scrollMetrics(viewport: number, content: number, track: number, offset: number) {
  const range = Math.max(0, content - viewport)
  const thumb = Math.min(track, Math.max(32, content > 0 ? track * viewport / content : track))
  const travel = Math.max(0, track - thumb)
  const position = range > 0 ? Math.min(range, Math.max(0, offset)) / range * travel : 0
  return { range, thumb, travel, position }
}
