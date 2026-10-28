import { useLayoutEffect, useRef } from 'react'
import { scrollMetrics } from '@/lib/scroll-metrics'

export default function WindowScrollbar() {
  const track = useRef<HTMLDivElement>(null)
  const thumb = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const rail = track.current!, handle = thumb.current!
    let frame = 0
    let idleTimer: ReturnType<typeof setTimeout> | undefined
    let drag: { pointer: number; grab: number } | null = null
    let geometry = { range: 0, thumb: 0, travel: 0, height: 0, top: 0, locked: false }
    const paint = (offset: number) => {
      const { range, travel } = geometry
      const scroll = Math.max(0, Math.min(range, offset))
      const scale = window.devicePixelRatio || 1
      handle.style.top = `${Math.min(travel, Math.round((range ? scroll / range * travel : 0) * scale) / scale)}px`
      handle.setAttribute('aria-valuenow', String(Math.round(scroll)))
    }
    const reveal = () => {
      rail.dataset.active = 'true'
      clearTimeout(idleTimer)
      idleTimer = setTimeout(() => { rail.dataset.active = 'false' }, 1200)
    }
    const measure = () => {
      frame = 0
      const root = document.documentElement
      const rect = rail.getBoundingClientRect()
      const locked = getComputedStyle(document.body).overflowY === 'hidden'
      const next = scrollMetrics(root.clientHeight, root.scrollHeight, rect.height, window.scrollY)
      const scale = window.devicePixelRatio || 1
      const size = Math.min(rect.height, Math.round(next.thumb * scale) / scale)
      geometry = { range: next.range, thumb: size, travel: rect.height - size, height: rect.height, top: rect.top, locked }
      const visible = next.range > 0 && !locked
      handle.style.height = `${size}px`
      handle.tabIndex = visible ? 0 : -1
      handle.setAttribute('aria-valuemax', String(Math.round(next.range)))
      rail.dataset.visible = String(visible)
      paint(window.scrollY)
    }
    const schedule = () => { if (!frame) frame = requestAnimationFrame(measure) }
    // Position updates use cached layout and run in the same input/scroll event.
    // Only size changes need measurement; dragging never waits for React or a second frame.
    const onScroll = () => { paint(window.scrollY); reveal() }
    const scrollTo = (top: number) => {
      window.scrollTo({ top: Math.max(0, Math.min(geometry.range, top)), behavior: 'instant' })
      onScroll()
    }
    const move = (event: PointerEvent) => {
      if (!drag || event.pointerId !== drag.pointer || geometry.locked) return
      const { top, thumb, travel, range } = geometry
      if (travel) scrollTo((event.clientY - top - drag.grab * thumb) / travel * range)
    }
    const down = (event: PointerEvent) => {
      if (event.button !== 0) return
      cancelAnimationFrame(frame)
      measure()
      const { range, travel, locked } = geometry
      if (!range || !travel || locked) return
      event.preventDefault()
      const onThumb = handle.contains(event.target as Node)
      const grab = onThumb ? (event.clientY - handle.getBoundingClientRect().top) / geometry.thumb : .5
      drag = { pointer: event.pointerId, grab: Math.max(0, Math.min(1, grab)) }
      rail.setPointerCapture(event.pointerId)
      rail.dataset.dragging = 'true'
      reveal()
      if (!onThumb) move(event)
    }
    const end = () => {
      const pointer = drag?.pointer
      drag = null
      rail.dataset.dragging = 'false'
      if (pointer !== undefined && rail.hasPointerCapture(pointer)) rail.releasePointerCapture(pointer)
    }
    const key = (event: KeyboardEvent) => {
      const step = window.innerHeight * .85
      const targets: Record<string, number> = {
        ArrowDown: window.scrollY + 48, ArrowUp: window.scrollY - 48,
        PageDown: window.scrollY + step, PageUp: window.scrollY - step,
        Home: 0, End: geometry.range,
      }
      if (event.key in targets) { event.preventDefault(); scrollTo(targets[event.key]) }
    }
    const resize = new ResizeObserver(schedule)
    resize.observe(document.body)
    resize.observe(document.documentElement)
    resize.observe(rail)
    const attributes = new MutationObserver(schedule)
    attributes.observe(document.body, { attributes: true, attributeFilter: ['style', 'data-scroll-locked'] })
    rail.addEventListener('pointerdown', down)
    rail.addEventListener('pointermove', move)
    rail.addEventListener('pointerup', end)
    rail.addEventListener('pointercancel', end)
    rail.addEventListener('lostpointercapture', end)
    handle.addEventListener('keydown', key)
    window.addEventListener('blur', end)
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', schedule)
    window.visualViewport?.addEventListener('resize', schedule)
    measure()
    return () => {
      end()
      cancelAnimationFrame(frame)
      clearTimeout(idleTimer)
      resize.disconnect()
      attributes.disconnect()
      rail.removeEventListener('pointerdown', down)
      rail.removeEventListener('pointermove', move)
      rail.removeEventListener('pointerup', end)
      rail.removeEventListener('pointercancel', end)
      rail.removeEventListener('lostpointercapture', end)
      handle.removeEventListener('keydown', key)
      window.removeEventListener('blur', end)
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', schedule)
      window.visualViewport?.removeEventListener('resize', schedule)
    }
  }, [])
  return (
    <div ref={track} className="window-scrollbar" data-visible="false">
      <div className="window-scrollbar-visual">
        <div ref={thumb} className="window-scrollbar-thumb" role="scrollbar" aria-label="页面滚动"
          aria-controls="root" aria-orientation="vertical" aria-valuemin={0}
          aria-valuemax={0} aria-valuenow={0} tabIndex={-1} />
      </div>
    </div>
  )
}
