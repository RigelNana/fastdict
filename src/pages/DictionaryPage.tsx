import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import { Link, useLocation, useMatch, useNavigate, useSearchParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  BookOpenText,
  Check,
  ChevronDown,
  ChevronsDownUp,
  ChevronsUpDown,
  ExternalLink,
  House,
  MoreHorizontal,
  X,
} from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Command, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Accordion } from '@/components/ui/accordion'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Skeleton } from '@/components/ui/skeleton'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import EntrySection, { DictionaryIcon } from '@/components/EntrySection'
import { api, useBootstrap } from '@/lib/api'
import type { Settings, SuggestionResponse } from '@/lib/types'
import { cn } from '@/lib/utils'
import { pendingSuggestions, normalizeHeadword as normalize, type SuggestionDisplay } from '@/lib/suggestion-cache'
import { readWordTrail, visitWord } from '@/lib/word-navigation'

function HighlightedWord({ word, query, active }: { word: string; query: string; active: boolean }) {
  if (!active) return <>{word}</>
  const normalizedQuery = normalize(query)
  if (!normalizedQuery) return <>{word}</>
  const lowerWord = word.toLowerCase()
  if (lowerWord.startsWith(normalizedQuery) && normalizedQuery.length <= word.length) {
    return (
      <>
        <span className="font-semibold text-foreground">{word.slice(0, normalizedQuery.length)}</span>
        <span className="text-foreground/80">{word.slice(normalizedQuery.length)}</span>
      </>
    )
  }
  return <>{word}</>
}

export default function DictionaryPage({ theme, visible = true }: { theme: 'light' | 'dark'; visible?: boolean }) {
  const bootstrap = useBootstrap()
  const client = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const match = useMatch('/word/:word')
  const word = match?.params.word ?? ''
  const navigationState = location.state as { preserve_search?: boolean; word_visit_key?: string } | null
  const positionKey = navigationState?.word_visit_key ?? location.key
  const [wordTrail, setWordTrail] = useState(() => {
    try { return readWordTrail(sessionStorage.getItem('fastdict.word-trail')) }
    catch { return readWordTrail(null) }
  })
  const previousWord = wordTrail.entries[wordTrail.index - 1]
  useLayoutEffect(() => {
    if (!word) return
    setWordTrail((trail) =>
      visitWord(trail, {
        key: positionKey,
        word,
        url: location.pathname + location.search,
      }),
    )
  }, [word, positionKey, location.pathname, location.search])
  useEffect(() => {
    try { sessionStorage.setItem('fastdict.word-trail', JSON.stringify(wordTrail)) }
    catch { /* Navigation remains available when session storage is unavailable. */ }
  }, [wordTrail])

  const [parameters] = useSearchParams()
  const [localGroup, setLocalGroup] = useState(() => sessionStorage.getItem('fastdict.group') ?? '')
  const group =
    parameters.get('group') ??
    (localGroup || bootstrap.data?.settings.application.default_group || 'all')
  const preferred = parameters.get('dictionary')
  const groupResolved = parameters.has('group') || Boolean(localGroup) || Boolean(bootstrap.data)
  const anchor = parameters.get('anchor') ?? undefined

  const [query, setQuery] = useState(word)
  const [active, setActive] = useState(false)
  const [candidateValue, setCandidateValue] = useState('')
  const [composing, setComposing] = useState(false)
  const [fulltextQuery, setFulltextQuery] = useState(() => normalize(word))
  const [mode, setMode] = useState<'headword' | 'fulltext'>('headword')
  const pendingSubmission = useRef<string | null>(null)
  const searchContext = useRef('')
  searchContext.current = JSON.stringify([query, group, mode, location.key, visible, active])

  const [expanded, setExpanded] = useState<string[]>([])
  const expandedRef = useRef(expanded)
  expandedRef.current = expanded
  const [external, setExternal] = useState<string | null>(null)

  const input = useRef<HTMLInputElement>(null)
  const searchShell = useRef<HTMLElement>(null)
  const candidateArea = useRef<HTMLDivElement>(null)
  const candidateHeight = useRef<{ scope: string; height: number } | null>(null)
  const suppressFocus = useRef(false)

  const appearance = bootstrap.data?.settings.appearance
  const animated = appearance?.motion !== false
  const isEntry = Boolean(word)
  const hasQuery = Boolean(query.trim())
  const panelOpen = active && hasQuery

  useLayoutEffect(() => {
    input.current?.setAttribute('aria-expanded', String(panelOpen))
  }, [panelOpen])

  useEffect(() => {
    if (mode !== 'fulltext' || composing) return
    const timer = window.setTimeout(() => setFulltextQuery(normalize(query)), 130)
    return () => window.clearTimeout(timer)
  }, [query, mode, composing])

  useLayoutEffect(() => {
    if (!visible) return
    const viewport = window.visualViewport
    const updateMaxHeight = () => {
      const vh = Math.round(viewport?.height ?? window.innerHeight)
      const shellBottom = Math.round(searchShell.current?.getBoundingClientRect().bottom ?? 120)
      const available = Math.max(140, Math.min(440, vh - shellBottom - 20))
      document.documentElement.style.setProperty('--candidate-available-height', `${available}px`)
    }
    updateMaxHeight()
    viewport?.addEventListener('resize', updateMaxHeight)
    window.addEventListener('resize', updateMaxHeight)
    return () => {
      viewport?.removeEventListener('resize', updateMaxHeight)
      window.removeEventListener('resize', updateMaxHeight)
    }
  }, [visible, isEntry, panelOpen])

  useLayoutEffect(() => {
    if (!(location.state as { preserve_search?: boolean } | null)?.preserve_search) {
      suppressFocus.current = true
      input.current?.blur()
      setQuery(word)
      setActive(false)
      setCandidateValue('')
      requestAnimationFrame(() => { suppressFocus.current = false })
    }
    document.title = word ? `${word} · fastdict` : 'fastdict · 查词'
  }, [word, location.key])

  const beginSearch = useCallback(() => {
    setActive(true)
    requestAnimationFrame(() => {
      input.current?.focus({ preventScroll: true })
      input.current?.select()
    })
  }, [])

  const dismissSearch = useCallback(() => {
    setActive(false)
    if (isEntry) setQuery(word)
    input.current?.blur()
  }, [isEntry, word])

  useEffect(() => {
    if (!visible) return
    const keydown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement
      if (event.defaultPrevented || target.closest('[data-slot="dropdown-menu-content"]')) return
      const editable =
        ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) || target.isContentEditable
      if (
        (event.key.toLowerCase() === 'k' && (event.metaKey || event.ctrlKey)) ||
        (event.key === '/' && !editable)
      ) {
        event.preventDefault()
        beginSearch()
      }
      if (event.key === 'Escape' && active && !event.isComposing) {
        dismissSearch()
      }
    }
    window.addEventListener('keydown', keydown)
    return () => window.removeEventListener('keydown', keydown)
  }, [active, beginSearch, dismissSearch, visible])

  useEffect(() => {
    if (!active || !visible) return
    const dismiss = (event: PointerEvent) => {
      const target = event.target as Element
      if (target.closest('.search-shell, [data-slot="select-content"], [data-slot="dropdown-menu-content"]')) return
      dismissSearch()
    }
    const articleFocus = () => dismissSearch()
    document.addEventListener('pointerdown', dismiss)
    window.addEventListener('fastdict:article-focus', articleFocus)
    return () => {
      document.removeEventListener('pointerdown', dismiss)
      window.removeEventListener('fastdict:article-focus', articleFocus)
    }
  }, [active, dismissSearch, visible])

  const allDictionaries = bootstrap.data?.dictionaries ?? []
  const selectedGroup = bootstrap.data?.groups.find((item) => item.id === group)
  const scopedDictionaries = useMemo(() => {
    const available = allDictionaries.filter((dictionary) => dictionary.enabled)
    if (group === 'all') return available
    if (!selectedGroup) return []
    const ordered = selectedGroup.dictionary_ids.flatMap(
      (id) => available.find((item) => item.id === id) ?? [],
    )
    if (!selectedGroup.sort.length) return ordered
    return ordered
      .map((dictionary, index) => ({ dictionary, index }))
      .sort((a, b) => {
        for (const rule of selectedGroup.sort) {
          let comparison = 0
          if (rule.field === 'entries') comparison = a.dictionary.entries - b.dictionary.entries
          else {
            const left =
              rule.field === 'name'
                ? a.dictionary.name.toLowerCase()
                : a.dictionary.created
            const right =
              rule.field === 'name'
                ? b.dictionary.name.toLowerCase()
                : b.dictionary.created
            comparison = left < right ? -1 : left > right ? 1 : 0
          }
          if (comparison) return rule.direction === 'asc' ? comparison : -comparison
        }
        return a.index - b.index
      })
      .map((item) => item.dictionary)
  }, [allDictionaries, group, selectedGroup])

  const suggestionQuery = mode === 'headword' ? normalize(query) : fulltextQuery
  const suggestionKey = ['suggest', suggestionQuery, group, mode] as const
  const suggestions = useQuery<SuggestionDisplay>({
    queryKey: suggestionKey,
    queryFn: ({ signal }) =>
      api<SuggestionResponse>(
        `/suggest?q=${encodeURIComponent(suggestionQuery)}&group=${encodeURIComponent(group)}&mode=${mode}&limit=30`,
        { signal },
      ),
    enabled: visible && !composing && Boolean(suggestionQuery) && groupResolved,
    staleTime: 30_000,
    placeholderData: (previous, previousQuery) =>
      pendingSuggestions(previous, previousQuery?.queryKey, suggestionKey),
  })
  const hits = useQuery({
    queryKey: ['suggest', normalize(word), group, 'headword'],
    queryFn: ({ signal }) =>
      api<SuggestionResponse>(
        `/suggest?q=${encodeURIComponent(normalize(word))}&group=${encodeURIComponent(group)}&mode=headword&limit=30`,
        { signal },
      ),
    enabled: isEntry && groupResolved,
    staleTime: 30_000,
  })
  const exactIds = new Set(
    hits.data?.items
      .filter((item) => item.match_kind === 'exact' && normalize(item.word) === normalize(word))
      .flatMap((item) => item.dictionary_ids) ?? [],
  )
  const dictionaries = scopedDictionaries.filter((item) => exactIds.has(item.id))
  const firstId = dictionaries.find((item) => item.id === preferred)?.id ?? dictionaries[0]?.id

  useEffect(() => {
    if (!firstId) return
    const key = `fastdict.expanded.${group}`
    try {
      const remembered = JSON.parse(sessionStorage.getItem(key) ?? 'null') as string[] | null
      setExpanded(remembered ?? [firstId])
    } catch {
      setExpanded([firstId])
    }
    if (preferred)
      setExpanded((value) => (value.includes(preferred) ? value : [...value, preferred]))
  }, [firstId, group, preferred])

  useLayoutEffect(() => {
    history.scrollRestoration = 'manual'
    if ((location.state as { preserve_search?: boolean } | null)?.preserve_search) return
    const key = `fastdict.position.${positionKey}`
    let saved: { scroll: number; expanded: string[] } | null = null
    try {
      saved = JSON.parse(sessionStorage.getItem(key) ?? 'null')
    } catch {
      /* Ignore obsolete session state. */
    }
    if (saved?.expanded) setExpanded(saved.expanded)
    const scroll = saved?.scroll ?? 0
    let cancelled = false
    const restore = () => {
      if (!cancelled) window.scrollTo({ top: scroll, behavior: 'instant' })
    }
    const observer = scroll > 0 ? new ResizeObserver(restore) : null
    observer?.observe(document.body)
    const cancel = () => {
      cancelled = true
      observer?.disconnect()
    }
    window.addEventListener('wheel', cancel, { passive: true, once: true })
    window.addEventListener('touchstart', cancel, { passive: true, once: true })
    window.addEventListener('keydown', cancel, { once: true })
    const timer = window.setTimeout(cancel, 10_000)
    restore()
    return () => {
      observer?.disconnect()
      window.clearTimeout(timer)
      window.removeEventListener('wheel', cancel)
      window.removeEventListener('touchstart', cancel)
      window.removeEventListener('keydown', cancel)
      sessionStorage.setItem(
        key,
        JSON.stringify({ scroll: window.scrollY, expanded: expandedRef.current }),
      )
    }
  }, [location.key, positionKey])

  function changeExpanded(values: string[]) {
    setExpanded(values)
    sessionStorage.setItem(`fastdict.expanded.${group}`, JSON.stringify(values))
  }

  function jumpToDictionary(dictionaryId: string) {
    if (!expanded.includes(dictionaryId)) {
      changeExpanded([...expanded, dictionaryId])
    }
    requestAnimationFrame(() => {
      const section = document.getElementById(`dict-section-${dictionaryId}`)
      section?.scrollIntoView({ behavior: animated ? 'smooth' : 'instant', block: 'start' })
    })
  }

  function openWord(target: string, source?: string, fragment?: string) {
    const trimmed = target.trim()
    if (!trimmed || trimmed.length > 512) return
    sessionStorage.setItem(
      `fastdict.position.${positionKey}`,
      JSON.stringify({ scroll: window.scrollY, expanded }),
    )
    suppressFocus.current = true
    input.current?.blur()
    setActive(false)
    setQuery(trimmed)
    const search = new URLSearchParams({ group })
    if (source) search.set('dictionary', source)
    if (fragment) search.set('anchor', fragment)
    navigate(`/word/${encodeURIComponent(trimmed)}?${search}`)
    void api('/history', {
      method: 'POST',
      body: JSON.stringify({ word: trimmed, group_id: group }),
    })
      .then(() => client.invalidateQueries({ queryKey: ['bootstrap'] }))
      .catch((error) => toast.error(error.message))
    requestAnimationFrame(() => {
      suppressFocus.current = false
    })
  }

  function goToPreviousWord() {
    if (!previousWord) return
    navigate(previousWord.url, { replace: true, state: { word_visit_key: previousWord.key } })
  }

  function goHome() {
    suppressFocus.current = true
    input.current?.blur()
    setActive(false)
    setQuery('')
    navigate('/')
  }

  function changeGroup(value: string) {
    setLocalGroup(value)
    sessionStorage.setItem('fastdict.group', value)
    if (isEntry) {
      const search = new URLSearchParams({ group: value })
      navigate(`/word/${encodeURIComponent(word)}?${search}`, {
        replace: true,
        state: { preserve_search: active, word_visit_key: positionKey },
      })
    }
  }

  function followExternal(url: string) {
    try {
      const target = new URL(url)
      if (!['http:', 'https:'].includes(target.protocol)) {
        toast.error('不支持这种外部链接。')
        return
      }
      if (bootstrap.data?.settings.security.external_links === 'block') {
        toast.error('外部链接已在安全设置中禁用。')
        return
      }
      if (bootstrap.data?.settings.security.external_links === 'allow')
        window.open(target.href, '_blank', 'noopener,noreferrer')
      else setExternal(target.href)
    } catch {
      toast.error('词典提供了无效的外部链接。')
    }
  }

  async function toggleReading() {
    if (!bootstrap.data) return
    try {
      await api<Settings>('/settings', {
        method: 'PUT',
        body: JSON.stringify({
          ...bootstrap.data.settings,
          appearance: {
            ...bootstrap.data.settings.appearance,
            reading_mode: !appearance?.reading_mode,
          },
        }),
      })
      await client.invalidateQueries({ queryKey: ['bootstrap'] })
    } catch (error) {
      toast.error(error instanceof Error ? error.message : '设置保存失败。')
    }
  }

  const retainedResults = suggestions.isPlaceholderData && suggestions.data?.retained === true
  const queryPending = composing || normalize(query) !== suggestionQuery || suggestions.isPending || retainedResults
  const suggestionError = suggestions.data ? null : suggestions.error
  const resultItems = suggestions.data?.items ?? []
  const selectedValue = resultItems.some((item) => item.word === candidateValue)
    ? candidateValue
    : resultItems[0]?.word ?? ''

  useLayoutEffect(() => setCandidateValue(''), [query, group, mode])

  async function submitSearch() {
    if (composing) return
    const context = searchContext.current
    const term = normalize(query)
    if (!term || pendingSubmission.current === context) return
    pendingSubmission.current = context
    try {
      const pending = queryPending || suggestions.isPlaceholderData
      const result = pending
        ? await client.fetchQuery({
            queryKey: ['suggest', term, group, mode],
            queryFn: ({ signal }) =>
              api<SuggestionResponse>(
                `/suggest?q=${encodeURIComponent(term)}&group=${encodeURIComponent(group)}&mode=${mode}&limit=30`,
                { signal },
              ),
            staleTime: 30_000,
          })
        : suggestions.data
      if (context !== searchContext.current) return
      const item =
        (!pending && result?.items.find((entry) => entry.word === selectedValue)) ||
        result?.items[0]
      if (item) openWord(item.word)
    } catch (error) {
      if (context === searchContext.current)
        toast.error(error instanceof Error ? error.message : '搜索失败')
    } finally {
      if (pendingSubmission.current === context) pendingSubmission.current = null
    }
  }

  const waitingForSuggestions = queryPending || suggestions.isPlaceholderData
  const candidateScope = JSON.stringify([group, mode])
  const heldHeight =
    hasQuery && waitingForSuggestions && candidateHeight.current?.scope === candidateScope
      ? candidateHeight.current.height
      : undefined

  useLayoutEffect(() => {
    if (!panelOpen || !visible) {
      candidateHeight.current = null
      return
    }
    if (waitingForSuggestions) return
    const viewport = candidateArea.current?.querySelector<HTMLElement>('.candidate-list')
    if (!viewport) return
    const measure = () => {
      candidateHeight.current = {
        scope: candidateScope,
        height: viewport.getBoundingClientRect().height,
      }
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(viewport)
    return () => observer.disconnect()
  }, [panelOpen, visible, waitingForSuggestions, candidateScope])

  const [showSuggestionLoading, setShowSuggestionLoading] = useState(false)
  useLayoutEffect(() => {
    setShowSuggestionLoading(false)
    if (!queryPending || composing || !panelOpen) return
    const timer = window.setTimeout(() => setShowSuggestionLoading(true), 150)
    return () => window.clearTimeout(timer)
  }, [queryPending, composing, panelOpen, suggestionQuery, group, mode])

  const groupLabel =
    group === 'all'
      ? '全部词典'
      : selectedGroup?.name ?? (bootstrap.isPending ? '读取中…' : '分组不可用')
  const allExpanded =
    dictionaries.length > 0 && dictionaries.every((item) => expanded.includes(item.id))

  return (
    <div
      className={cn(
        'dictionary-app',
        isEntry ? 'entry-view' : 'home-view',
        appearance?.reading_mode && 'reading-mode',
      )}
      style={{ '--reading-content-width': `${appearance?.content_width ?? 840}px` } as CSSProperties}
    >
      <section
        ref={searchShell}
        className="search-shell"
        data-entry={isEntry}
        data-active={panelOpen}
        aria-label="查词"
      >
        <div className="search-shell-inner">
          <Command
            shouldFilter={false}
            value={selectedValue}
            onValueChange={setCandidateValue}
            loop
            className="search-command h-auto overflow-visible rounded-none bg-transparent"
            onKeyDownCapture={(event) => {
              if (event.target !== input.current || event.key !== 'Enter') return
              if (
                event.nativeEvent.isComposing ||
                event.nativeEvent.keyCode === 229 ||
                composing
              ) {
                event.stopPropagation()
                return
              }
              event.preventDefault()
              event.stopPropagation()
              void submitSearch()
            }}
          >
            <div className="search-row">
              {isEntry && (
                <div className="search-navigation" role="group" aria-label="词条导航">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label="返回上一个词条"
                        className="search-back search-menu-trigger"
                        disabled={!previousWord}
                        onClick={goToPreviousWord}
                      >
                        <ArrowLeft className="size-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom">返回上一个词条</TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label="返回首页"
                        className="search-home search-menu-trigger"
                        onClick={goHome}
                      >
                        <House className="size-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom">返回首页</TooltipContent>
                  </Tooltip>
                </div>
              )}

              <CommandInput
                ref={input}
                aria-label="搜索词条"
                aria-expanded={panelOpen}
                placeholder={mode === 'fulltext' ? '输入正文关键词检索…' : '输入单词或词组…'}
                value={query}
                onValueChange={(value) => {
                  setQuery(value)
                  setActive(true)
                }}
                onFocus={() => {
                  if (!suppressFocus.current) {
                    setActive(true)
                  }
                }}
                onCompositionStart={() => setComposing(true)}
                onCompositionEnd={() => setComposing(false)}
                autoComplete="off"
                autoCorrect="off"
                spellCheck={false}
                enterKeyHint="search"
                className="search-input"
              />

              {query && (
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="清空搜索"
                  className="search-clear"
                  onClick={() => {
                    setQuery('')
                    input.current?.focus()
                  }}
                >
                  <X className="size-4" />
                </Button>
              )}

              <div className="search-mode-toggle" role="group" aria-label="搜索模式">
                <button
                  type="button"
                  className="search-mode-pill"
                  data-active={mode === 'headword'}
                  onClick={() => {
                    setMode('headword')
                    input.current?.focus()
                  }}
                >
                  词头
                </button>
                <button
                  type="button"
                  className="search-mode-pill"
                  data-active={mode === 'fulltext'}
                  onClick={() => {
                    setMode('fulltext')
                    input.current?.focus()
                  }}
                >
                  全文
                </button>
              </div>

              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="search-group search-menu-trigger"
                    aria-label="当前查询分组"
                    onKeyDown={(event) => event.stopPropagation()}
                  >
                    <span className="truncate">{groupLabel}</span>
                    <ChevronDown className="size-3 opacity-65" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" onKeyDown={(event) => event.stopPropagation()}>
                  <DropdownMenuItem onSelect={() => changeGroup('all')}>
                    全部词典{group === 'all' && <Check className="ml-auto" />}
                  </DropdownMenuItem>
                  {bootstrap.data?.groups.map((item) => (
                    <DropdownMenuItem key={item.id} onSelect={() => changeGroup(item.id)}>
                      {item.name}
                      {group === item.id && <Check className="ml-auto" />}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>

              {isEntry && (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label="词条操作"
                      className="search-menu-trigger"
                    >
                      <MoreHorizontal className="size-4" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-44">
                    <DropdownMenuItem onClick={() => void toggleReading()}>
                      <BookOpenText />
                      {appearance?.reading_mode ? '退出阅读模式' : '专注阅读模式'}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onClick={() => changeExpanded(dictionaries.map((item) => item.id))}
                    >
                      <ArrowDown />
                      全部展开
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => changeExpanded([])}>
                      <ArrowUp />
                      全部收起
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </div>

            <div className="suggestion-panel" hidden={!panelOpen} inert={!panelOpen}>
              <ScrollArea
                ref={candidateArea}
                viewportClassName="candidate-list"
                style={
                  {
                    '--candidate-held-height':
                      heldHeight === undefined ? undefined : `${heldHeight}px`,
                  } as CSSProperties
                }
              >
                <CommandList
                  className="max-h-none overflow-visible"
                  aria-label="候选词条"
                  aria-busy={queryPending || suggestions.isPlaceholderData}
                >
                  {queryPending && resultItems.length === 0 && (
                    <div
                      className="space-y-3 px-4 py-5 min-h-24"
                      role="status"
                      aria-label={showSuggestionLoading ? '正在搜索' : undefined}
                    >
                      {showSuggestionLoading && (
                        <>
                          <Skeleton className="h-4 w-2/3" />
                          <Skeleton className="h-4 w-1/2" />
                        </>
                      )}
                    </div>
                  )}
                  {!queryPending && suggestionError && (
                    <div className="candidate-message text-destructive" role="alert">
                      {suggestionError.message}
                      <Button variant="ghost" size="sm" onClick={() => void suggestions.refetch()}>
                        重试
                      </Button>
                    </div>
                  )}
                  {!suggestionError &&
                    resultItems.map((item) => (
                      <CommandItem
                        key={item.word}
                        value={item.word}
                        className="candidate data-[disabled=true]:opacity-100"
                        disabled={queryPending}
                        onSelect={() => {
                          if (!queryPending) openWord(item.word)
                        }}
                      >
                        <div className="min-w-0 grow">
                          <div className="candidate-word">
                            <HighlightedWord
                              word={item.word}
                              query={query}
                              active={mode === 'headword' && item.match_kind !== 'fuzzy'}
                            />
                          </div>
                          {item.preview && (
                            <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                              {item.preview}
                            </p>
                          )}
                        </div>
                        {item.match_kind === 'fuzzy' ? (
                          <span className="candidate-count">近似</span>
                        ) : (
                          item.dictionary_ids.length > 1 && (
                            <span className="candidate-count">{item.dictionary_ids.length} 部</span>
                          )
                        )}
                        <ArrowRight className="size-3.5 candidate-arrow" />
                      </CommandItem>
                    ))}
                  {!queryPending &&
                    !suggestionError &&
                    suggestionQuery &&
                    resultItems.length === 0 && (
                      <div className="candidate-message" role="status">
                        {mode === 'fulltext' ? (
                          <div className="space-y-1.5">
                            <div>未找到包含该词的词条正文</div>
                            <Link
                              to="/settings/dictionaries"
                              className="text-xs underline hover:text-foreground"
                            >
                              前往词典设置建立全文索引
                            </Link>
                          </div>
                        ) : (
                          '无结果'
                        )}
                      </div>
                    )}
                </CommandList>
              </ScrollArea>
            </div>
          </Command>

          {!isEntry && !panelOpen && (bootstrap.error || (!bootstrap.isPending && allDictionaries.length === 0)) && (
            <div className="home-status">
              {bootstrap.error ? (
                <Link to="/settings/system" className="text-destructive underline">
                  连接词典服务
                </Link>
              ) : (
                <Link to="/settings/dictionaries" className="underline">
                  前往导入词典
                </Link>
              )}
            </div>
          )}
        </div>
      </section>

      {isEntry && (
        <main
          className="entry-content"
          style={{ maxWidth: appearance?.reading_mode ? appearance.content_width : undefined }}
        >
          {dictionaries.length > 0 ? (
            <header className="entry-header">
              <div className="entry-header-top">
                <h1 className="entry-headword">{word}</h1>
                {dictionaries.length > 1 && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="entry-toggle-all text-xs text-muted-foreground hover:text-foreground"
                    onClick={() =>
                      changeExpanded(allExpanded ? [] : dictionaries.map((item) => item.id))
                    }
                  >
                    {allExpanded ? (
                      <>
                        <ChevronsDownUp className="size-3.5" />
                        全部收起
                      </>
                    ) : (
                      <>
                        <ChevronsUpDown className="size-3.5" />
                        全部展开
                      </>
                    )}
                  </Button>
                )}
              </div>
              {dictionaries.length > 1 && (
                <nav className="dictionary-jump-bar" aria-label="命中词典快速定位">
                  {dictionaries.map((dictionary) => {
                    const isExpanded = expanded.includes(dictionary.id)
                    return (
                      <button
                        key={dictionary.id}
                        type="button"
                        className="dictionary-jump-pill"
                        data-expanded={isExpanded}
                        title={dictionary.name}
                        onClick={() => jumpToDictionary(dictionary.id)}
                      >
                        <DictionaryIcon dictionary={dictionary} />
                        <span className="truncate">{dictionary.name}</span>
                      </button>
                    )
                  })}
                </nav>
              )}
            </header>
          ) : (
            <h1 className="sr-only">{word}</h1>
          )}

          {(bootstrap.isPending || hits.isPending) && !bootstrap.error && dictionaries.length === 0 && (
            <div className="space-y-5 py-10" role="status" aria-label="正在查询词典">
              <Skeleton className="h-6 w-44" />
              <Skeleton className="h-4 w-4/5" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-3/5" />
            </div>
          )}

          {(bootstrap.error || hits.error) && dictionaries.length === 0 && (
            <div className="reading-message" role="alert">
              <p>{(bootstrap.error ?? hits.error)?.message}</p>
              <Button
                variant="outline"
                onClick={() => {
                  void bootstrap.refetch()
                  void hits.refetch()
                }}
              >
                重新连接
              </Button>
              <Button variant="outline" asChild>
                <Link to="/settings/system">连接设置</Link>
              </Button>
            </div>
          )}

          {bootstrap.data && hits.isSuccess && dictionaries.length === 0 && (
            <div className="reading-message" role="status">
              无结果
            </div>
          )}

          {appearance && (
            <Accordion type="multiple" value={expanded} onValueChange={changeExpanded}>
              {dictionaries.map((dictionary) => (
                <EntrySection
                  key={dictionary.id}
                  dictionary={dictionary}
                  word={word}
                  expanded={expanded.includes(dictionary.id)}
                  appearance={appearance}
                  theme={theme}
                  anchor={dictionary.id === preferred ? anchor : undefined}
                  onNavigate={(target, source, fragment) => openWord(target, source, fragment)}
                  onExternal={followExternal}
                />
              ))}
            </Accordion>
          )}
        </main>
      )}

      <AlertDialog
        open={Boolean(external)}
        onOpenChange={(open) => {
          if (!open) setExternal(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>打开词典中的外部链接？</AlertDialogTitle>
            <AlertDialogDescription className="break-all">{external}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>留在这里</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (external) window.open(external, '_blank', 'noopener,noreferrer')
                setExternal(null)
              }}
            >
              打开链接
              <ExternalLink className="size-4" />
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
