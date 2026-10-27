import { useEffect, useRef, useState, type SetStateAction } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { getConnection } from './api'
import { enqueueSave } from './save-queue'

const drafts = new Map<string, unknown>()

export function useAutosave<T>({ value, resource, save, valid = () => true, project = (v) => v }: {
  value: T
  resource: string
  save: (value: T) => Promise<unknown>
  valid?: (value: T) => boolean
  project?: (value: T) => unknown
}) {
  const client = useQueryClient()
  const connection = useRef(getConnection()).current
  const key = `${getConnection().base_url}:${resource}`
  const [draft, render] = useState<T>(() => (drafts.get(key) as T | undefined) ?? value)
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const latest = useRef(draft)
  const options = useRef({ save, valid, project })
  options.current = { save, valid, project }
  const fingerprint = (v: T) => JSON.stringify(options.current.project(v))
  const acknowledged = useRef(fingerprint(value))
  const mounted = useRef(true)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const running = useRef<Promise<boolean> | null>(null)

  function flush(): Promise<boolean> {
    clearTimeout(timer.current)
    if (running.current) return running.current.then((ok) =>
      ok && fingerprint(latest.current) !== acknowledged.current ? flushRef.current() : ok,
    )
    running.current = (async () => {
      while (fingerprint(latest.current) !== acknowledged.current) {
        const snapshot = latest.current
        if (!options.current.valid(snapshot)) return false
        const stamp = fingerprint(snapshot)
        const write = options.current.save
        if (mounted.current) { setPending(true); setError('') }
        try {
          await enqueueSave(key, () => {
            const active = getConnection()
            if (active.base_url !== connection.base_url || active.token !== connection.token) {
              throw new Error('服务连接已更改，请回到原连接重试。')
            }
            return write(snapshot)
          })
          acknowledged.current = stamp
          if (drafts.has(key) && fingerprint(drafts.get(key) as T) === stamp) drafts.delete(key)
          await client.invalidateQueries({ queryKey: ['bootstrap'] })
        } catch (cause) {
          const message = cause instanceof Error ? cause.message : '保存失败，请重试。'
          if (mounted.current) setError(message)
          toast.error(message)
          return false
        }
      }
      return true
    })().finally(() => {
      running.current = null
      if (mounted.current) setPending(false)
    })
    return running.current
  }
  const flushRef = useRef(flush)
  flushRef.current = flush

  function change(next: SetStateAction<T>, immediate = false) {
    const value = typeof next === 'function' ? (next as (previous: T) => T)(latest.current) : next
    latest.current = value
    drafts.set(key, value)
    render(value)
    setError('')
    clearTimeout(timer.current)
    if (immediate) void flushRef.current()
    else timer.current = setTimeout(() => void flushRef.current(), 400)
  }
  function replace(value: T) {
    clearTimeout(timer.current)
    acknowledged.current = fingerprint(value)
    drafts.delete(key)
    latest.current = value
    if (mounted.current) render(value)
  }
  useEffect(() => {
    if (!running.current && fingerprint(latest.current) === acknowledged.current) replace(value)
  }, [value])
  useEffect(() => {
    mounted.current = true
    if (fingerprint(latest.current) !== acknowledged.current) void flushRef.current()
    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (fingerprint(latest.current) !== acknowledged.current) {
        void flushRef.current()
        event.preventDefault()
      }
    }
    window.addEventListener('beforeunload', beforeUnload)
    return () => {
      mounted.current = false
      clearTimeout(timer.current)
      window.removeEventListener('beforeunload', beforeUnload)
      void flushRef.current()
    }
  }, [])
  return { draft, change, replace, flush, pending, error, valid: valid(draft) }
}
