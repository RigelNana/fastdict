import { useEffect, useRef, useState } from 'react'
import { scrollMetrics } from '@/lib/scroll-metrics'

export default function WindowScrollbar() {
  const track = useRef<HTMLDivElement>(null)
  const drag = useRef<{ y: number; scroll: number } | null>(null)
  const [metrics, setMetrics] = useState({ range: 0, thumb: 0, travel: 0, position: 0, scroll: 0, locked: false })
  useEffect(() => {
    let frame = 0
    const measure = () => {
      frame = 0
      const root = document.documentElement
      const height = track.current?.clientHeight ?? 0
      const locked = getComputedStyle(document.body).overflowY === 'hidden'
      setMetrics({ ...scrollMetrics(root.clientHeight, root.scrollHeight, height, window.scrollY), scroll: window.scrollY, locked })
    }
    const schedule = () => { if (!frame) frame = requestAnimationFrame(measure) }
    const resize = new ResizeObserver(schedule)
    resize.observe(document.body)
    resize.observe(document.documentElement)
    const attributes = new MutationObserver(schedule)
    attributes.observe(document.body, { attributes: true, attributeFilter: ['style', 'data-scroll-locked'] })
    window.addEventListener('scroll', schedule, { passive: true })
    window.addEventListener('resize', schedule)
    measure()
    return () => {
      cancelAnimationFrame(frame)
      resize.disconnect()
      attributes.disconnect()
      window.removeEventListener('scroll', schedule)
      window.removeEventListener('resize', schedule)
    }
  }, [])
  return (
    <div ref={track} className="window-scrollbar" data-visible={metrics.range > 0 && !metrics.locked}
      onPointerDown={(event) => {
        if (event.button !== 0 || !metrics.range || !metrics.travel) return
        event.preventDefault()
        event.currentTarget.setPointerCapture(event.pointerId)
        if (!(event.target as Element).closest('.window-scrollbar-thumb')) {
          const y = event.clientY - event.currentTarget.getBoundingClientRect().top - metrics.thumb / 2
          window.scrollTo({ top: Math.max(0, Math.min(metrics.travel, y)) / metrics.travel * metrics.range, behavior: 'instant' })
        }
        drag.current = { y: event.clientY, scroll: window.scrollY }
      }}
      onPointerMove={(event) => {
        if (drag.current && metrics.travel) window.scrollTo({
          top: drag.current.scroll + (event.clientY - drag.current.y) / metrics.travel * metrics.range,
          behavior: 'instant',
        })
      }}
      onLostPointerCapture={() => { drag.current = null }}
      onPointerUp={() => { drag.current = null }}
    >
      <div className="window-scrollbar-thumb" role="scrollbar" aria-label="页面滚动"
        aria-controls="root" aria-orientation="vertical" aria-valuemin={0}
        aria-valuemax={Math.round(metrics.range)} aria-valuenow={Math.round(Math.max(0, Math.min(metrics.range, metrics.scroll)))}
        tabIndex={metrics.range > 0 && !metrics.locked ? 0 : -1}
        style={{ height: metrics.thumb, transform: `translateY(${metrics.position}px)` }}
        onKeyDown={(event) => {
          const step = window.innerHeight * .85
          const targets: Record<string, number> = {
            ArrowDown: window.scrollY + 48, ArrowUp: window.scrollY - 48,
            PageDown: window.scrollY + step, PageUp: window.scrollY - step,
            Home: 0, End: metrics.range,
          }
          if (event.key in targets) { event.preventDefault(); window.scrollTo({ top: targets[event.key], behavior: 'instant' }) }
        }}
      />
    </div>
  )
}
