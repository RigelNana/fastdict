import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, BookOpenText, RotateCcw } from 'lucide-react'
import { toast } from 'sonner'
import { AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { api, apiBlob } from '@/lib/api'
import type { Article, Dictionary, LookupResponse, Settings } from '@/lib/types'
import { articleThemeResource, fetchArticleThemeResource } from '@/lib/article-theme'
import darkreaderUrl from 'darkreader/darkreader.js?url'

function ArticleFrame({
  article,
  title,
  appearance,
  theme,
  onNavigate,
  onExternal,
  anchor,
}: {
  article: Article
  title: string
  appearance: Settings['appearance']
  theme: 'light' | 'dark'
  onNavigate: (word: string, anchor?: string) => void
  onExternal: (url: string) => void
  anchor?: string
}) {
  const frame = useRef<HTMLIFrameElement>(null)
  const client = useQueryClient()
  // Set the first paint's theme in the document. Later theme changes use the bridge,
  // keeping the iframe, selection, audio and dictionary scripts mounted.
  const srcDoc = useMemo(() => article.html.replace('<html ', `<html data-fastdict-theme="${theme}" `), [article.html])
  const [height, setHeight] = useState(120)
  useEffect(() => {
    const clear = () => frame.current?.contentWindow?.postMessage({ type: 'fastdict:clear-selection' }, '*')
    const otherFrame = (event: Event) => {
      if ((event as CustomEvent).detail !== frame.current?.contentWindow) clear()
    }
    document.addEventListener('pointerdown', clear)
    window.addEventListener('fastdict:article-focus', otherFrame)
    return () => {
      document.removeEventListener('pointerdown', clear)
      window.removeEventListener('fastdict:article-focus', otherFrame)
    }
  }, [])
  useEffect(() => {
    let live = true
    const pendingThemeResources = new Set<number>()
    function onMessage(event: MessageEvent) {
      if (
        event.source !== frame.current?.contentWindow ||
        !event.data ||
        typeof event.data !== 'object'
      )
        return
      const data = event.data as Record<string, unknown>
      if (data.type === 'fastdict:theme-resource') {
        if (typeof data.requestId !== 'number' || !Number.isSafeInteger(data.requestId) ||
          data.requestId < 1 || typeof data.url !== 'string' || pendingThemeResources.has(data.requestId)) return
        const target = frame.current!.contentWindow!
        const requestId = data.requestId
        const url = articleThemeResource(article.html, data.url)
        if (!url || pendingThemeResources.size >= 32) {
          target.postMessage({ type: 'fastdict:theme-resource-result', requestId, error: true }, '*')
          return
        }
        pendingThemeResources.add(requestId)
        void client.fetchQuery({
          queryKey: ['article-theme-resource', url],
          queryFn: () => fetchArticleThemeResource(url),
          staleTime: 5 * 60_000, gcTime: 10 * 60_000,
        }).then((resource) => {
          if (!live || frame.current?.contentWindow !== target) return
          const body = resource.body.slice(0)
          target.postMessage({ type: 'fastdict:theme-resource-result', requestId, body, contentType: resource.contentType }, '*', [body])
        }).catch(() => {
          if (live && frame.current?.contentWindow === target)
            target.postMessage({ type: 'fastdict:theme-resource-result', requestId, error: true }, '*')
        }).finally(() => pendingThemeResources.delete(requestId))
      }
    }
    window.addEventListener('message', onMessage)
    return () => { live = false; window.removeEventListener('message', onMessage) }
  }, [article.html, client])
  useEffect(() => {
    function onMessage(event: MessageEvent) {
      if (event.source !== frame.current?.contentWindow || !event.data || typeof event.data !== 'object') return
      const data = event.data as Record<string, unknown>
      // A collapsed frame keeps its measured height and cannot navigate the reader.
      if (!frame.current?.getClientRects().length) return
      if (data.type === 'fastdict:focus') {
        window.dispatchEvent(new CustomEvent('fastdict:article-focus', { detail: event.source }))
      }
      if (
        data.type === 'fastdict:height' &&
        typeof data.height === 'number' &&
        Number.isFinite(data.height)
      )
        setHeight(Math.min(500_000, Math.max(40, data.height)))
      if (
        data.type === 'fastdict:navigate' &&
        typeof data.word === 'string' &&
        data.word.length <= 512
      )
        onNavigate(
          data.word,
          typeof data.anchor === 'string' ? data.anchor.slice(0, 512) : undefined,
        )
      if (
        data.type === 'fastdict:external' &&
        typeof data.url === 'string' &&
        data.url.length <= 4096
      )
        onExternal(data.url)
      if (data.type === 'fastdict:error' && typeof data.message === 'string' && data.message.length <= 300) toast.error(data.message)
      if (
        data.type === 'fastdict:anchor' &&
        typeof data.offset === 'number' &&
        Number.isFinite(data.offset) &&
        data.offset >= 0 &&
        data.offset <= height
      ) {
        const top = frame.current!.getBoundingClientRect().top + window.scrollY + data.offset - 96
        window.scrollTo({ top, behavior: appearance.motion ? 'smooth' : 'instant' })
      }
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [onNavigate, onExternal, height, appearance.motion])
  function sendStyle() {
    frame.current?.contentWindow?.postMessage({ type: 'fastdict:style', ...appearance, theme,
      themeRuntimeUrl: new URL(darkreaderUrl, window.location.href).href }, '*')
    if (anchor) frame.current?.contentWindow?.postMessage({ type: 'fastdict:anchor', anchor }, '*')
  }
  useEffect(sendStyle, [appearance, theme, anchor])
  return (
    <iframe
      ref={frame}
      title={title}
      srcDoc={srcDoc}
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      className="article-frame"
      style={{ height }}
      onLoad={sendStyle}
    />
  )
}

export function DictionaryIcon({ dictionary }: { dictionary: Dictionary }) {
  const [iconUrl, setIconUrl] = useState<string | null>(null)
  useEffect(() => {
    setIconUrl(null)
    const selected = dictionary.icon
    const resource = dictionary.resources.find(
      (item) =>
        item.enabled &&
        item.kind === 'image' &&
        (selected
          ? item.id === selected || item.key === selected || item.path === selected
          : /(?:^|\/)(?:icon|favicon)\.(?:png|svg|ico|webp)$/i.test(item.path)),
    )
    const candidates = resource
      ? [{ resource, path: '' }]
      : dictionary.resources
          .filter((item) => item.enabled && item.kind === 'mdd')
          .map((item) => ({ resource: item, path: selected || 'icon.png' }))
    if (!candidates.length) return
    let active = true
    let objectUrl: string | undefined
    void (async () => {
      for (const candidate of candidates) {
        try {
          const query = candidate.path ? `?path=${encodeURIComponent(candidate.path)}` : ''
          const blob = await apiBlob(
            `/dictionaries/${dictionary.id}/resources/${candidate.resource.id}/content${query}`,
          )
          if (!active) return
          objectUrl = URL.createObjectURL(blob)
          setIconUrl(objectUrl)
          return
        } catch {
          /* A missing icon in one mounted volume is not a reading error. */
        }
      }
    })()
    return () => {
      active = false
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [dictionary.id, dictionary.revision, dictionary.icon, dictionary.resources])
  return iconUrl ? (
    <img src={iconUrl} alt="" className="dictionary-icon object-contain" />
  ) : (
    <BookOpenText aria-hidden className="dictionary-icon text-primary/70" strokeWidth={1.5} />
  )
}

export default function EntrySection({
  dictionary,
  word,
  expanded,
  appearance,
  theme,
  onNavigate,
  onExternal,
  anchor,
}: {
  dictionary: Dictionary
  word: string
  expanded: boolean
  appearance: Settings['appearance']
  theme: 'light' | 'dark'
  onNavigate: (word: string, source: string, anchor?: string) => void
  onExternal: (url: string) => void
  anchor?: string
}) {
  const [openedWord, setOpenedWord] = useState<string | null>(null)
  useEffect(() => {
    if (expanded) setOpenedWord(word)
  }, [expanded, word])
  const result = useQuery({
    queryKey: [
      'entry',
      dictionary.id,
      dictionary.revision,
      word,
      dictionary.custom_css,
      dictionary.font_family,
      dictionary.trust_scripts,
      dictionary.typography_mode,
      dictionary.resources,
    ],
    queryFn: ({ signal }) =>
      api<LookupResponse>(
        `/lookup?dictionary=${encodeURIComponent(dictionary.id)}&q=${encodeURIComponent(word)}`,
        { signal },
      ),
    enabled: expanded,
    staleTime: 5 * 60_000,
    gcTime: 10 * 60_000,
  })
  return (
    <AccordionItem value={dictionary.id} className="dictionary-section border-0">
      <AccordionTrigger className="dictionary-heading hover:no-underline">
        <span className="flex min-w-0 items-center gap-3">
          <DictionaryIcon dictionary={dictionary} />
          <span className="truncate">{dictionary.name}</span>
        </span>
      </AccordionTrigger>
      <AccordionContent
        forceMount={openedWord === word ? true : undefined}
        hidden={!expanded}
        className="dictionary-body pb-0"
      >
        {result.isPending && (
          <div className="space-y-3 py-4" role="status" aria-label="正在载入词条">
            <Skeleton className="h-4 w-4/5" />
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-3/5" />
          </div>
        )}
        {result.error && (
          <div className="flex items-start gap-3 py-4 text-sm text-destructive" role="alert">
            <AlertCircle className="mt-0.5 size-4 shrink-0" />
            <div className="space-y-2">
              <p>{result.error.message}</p>
              <Button variant="outline" size="sm" onClick={() => void result.refetch()}>
                <RotateCcw className="size-3.5" />
                重试这部词典
              </Button>
            </div>
          </div>
        )}
        {result.data?.articles.length === 0 && (
          <p className="py-4 text-sm text-muted-foreground">这部词典没有该词条。</p>
        )}
        {result.data?.articles.map((article, index) => (
          <div key={index} className="article-part">
            <ArticleFrame
              article={article}
              title={`${dictionary.name} · ${word} · ${index + 1}`}
              appearance={appearance}
              theme={theme}
              onNavigate={(target, fragment) => onNavigate(target, dictionary.id, fragment)}
              onExternal={onExternal}
              anchor={index === 0 ? anchor : undefined}
            />
          </div>
        ))}
      </AccordionContent>
    </AccordionItem>
  )
}
