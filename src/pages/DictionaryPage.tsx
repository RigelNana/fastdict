import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import { Link, useLocation, useMatch, useNavigate, useSearchParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { motion, useReducedMotion } from 'motion/react'
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  BookOpenText,
  Check,
  ChevronDown,
  Clock3,
  ExternalLink,
  House,
  ListFilter,
  MoreHorizontal,
  Trash2,
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
  DropdownMenuSeparator,
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
import EntrySection from '@/components/EntrySection'
import { api, useBootstrap } from '@/lib/api'
import type { Dictionary, Settings, SuggestionResponse } from '@/lib/types'
import { cn } from '@/lib/utils'
import { pendingSuggestions, normalizeHeadword as normalize, type SuggestionDisplay } from '@/lib/suggestion-cache'
import { readWordTrail, visitWord } from '@/lib/word-navigation'
import { searchOffsets } from '@/lib/search-layout'


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
    setWordTrail(trail => visitWord(trail, {
      key: positionKey, word, url: location.pathname + location.search,
    }))
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
  const anchor = parameters.get('anchor') ?? undefined
  const [query, setQuery] = useState(word)
  const [active, setActive] = useState(false)
  const [candidateValue, setCandidateValue] = useState('')
  const [composing, setComposing] = useState(false)
  const [fulltextQuery, setFulltextQuery] = useState(() => normalize(word))
  const [mode, setMode] = useState<'headword' | 'fulltext'>('headword')
  const historySelection = useRef(0)
  const pendingSubmission = useRef<string | null>(null)
  const searchContext = useRef('')
  searchContext.current = JSON.stringify([query, group, mode, location.key, visible, active])
  const [expanded, setExpanded] = useState<string[]>([])
  const expandedRef = useRef(expanded)
  expandedRef.current = expanded
  const [external, setExternal] = useState<string | null>(null)
  const input = useRef<HTMLInputElement>(null)
  const searchBox = useRef<HTMLElement>(null)
  const candidateArea = useRef<HTMLDivElement>(null)
  const candidateHeight = useRef<{ scope: string; height: number } | null>(null)
  const [offsets, setOffsets] = useState(() => searchOffsets(window.visualViewport?.height ?? window.innerHeight, window.visualViewport?.offsetTop ?? 0, 92))
  const suppressFocus = useRef(false)
  const reducedMotion = useReducedMotion()
  const appearance = bootstrap.data?.settings.appearance
  const animated = appearance?.motion !== false && !reducedMotion
  const isEntry = Boolean(word)

  useLayoutEffect(() => {
    // cmdk assumes a permanently open list; this list also has a collapsed state.
    input.current?.setAttribute('aria-expanded', String(active))
  }, [active])

  useEffect(() => {
    if (mode !== 'fulltext' || composing) return
    const timer = window.setTimeout(() => setFulltextQuery(normalize(query)), 130)
    return () => window.clearTimeout(timer)
  }, [query, mode, composing])
  useLayoutEffect(() => {
    if (!visible) return
    const viewport = window.visualViewport
    const resize = () => {
      const height = viewport?.height ?? window.innerHeight
      const top = viewport?.offsetTop ?? 0
      const boxHeight = searchBox.current?.offsetHeight ?? 92
      document.documentElement.style.setProperty('--visual-height', `${height}px`)
      searchBox.current?.parentElement?.style.setProperty('--search-height', `${boxHeight}px`)
      setOffsets(searchOffsets(height, top, boxHeight))
    }
    viewport?.addEventListener('resize', resize)
    viewport?.addEventListener('scroll', resize)
    window.addEventListener('resize', resize)
    const observer = new ResizeObserver(resize)
    if (searchBox.current) observer.observe(searchBox.current)
    resize()
    return () => {
      viewport?.removeEventListener('resize', resize)
      viewport?.removeEventListener('scroll', resize)
      window.removeEventListener('resize', resize)
      observer.disconnect()
    }
  }, [visible])
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
        setActive(false)
        input.current?.blur()
      }
    }
    window.addEventListener('keydown', keydown)
    return () => window.removeEventListener('keydown', keydown)
  }, [active, beginSearch, visible])

  useEffect(() => {
    if (!active || !visible) return
    const dismiss = (event: PointerEvent) => {
      const target = event.target as Element
      if (target.closest('.search-positioner, [data-slot="select-content"], [data-slot="dropdown-menu-content"]')) return
      setActive(false)
      input.current?.blur()
    }
    const articleFocus = () => { setActive(false); input.current?.blur() }
    document.addEventListener('pointerdown', dismiss)
    window.addEventListener('fastdict:article-focus', articleFocus)
    return () => {
      document.removeEventListener('pointerdown', dismiss)
      window.removeEventListener('fastdict:article-focus', articleFocus)
    }
  }, [active, visible])

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
    // Prepare the current word before focus; reopening must not restart loading.
    enabled: visible && !composing && Boolean(suggestionQuery) && Boolean(bootstrap.data),
    staleTime: 30_000,
    placeholderData: (previous, previousQuery) => pendingSuggestions(previous, previousQuery?.queryKey, suggestionKey),
  })
  const hits = useQuery({
    // Search already identifies the dictionaries. Share its exact query/cache
    // instead of starting a second request before mounting the selected article.
    queryKey: ['suggest', normalize(word), group, 'headword'],
    queryFn: ({ signal }) =>
      api<SuggestionResponse>(
        `/suggest?q=${encodeURIComponent(normalize(word))}&group=${encodeURIComponent(group)}&mode=headword&limit=30`,
        { signal },
      ),
    enabled: isEntry && Boolean(bootstrap.data),
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
  async function openHistoryWord(target: string) {
    const context = searchContext.current
    const selection = ++historySelection.current
    const current = () => selection === historySelection.current && context === searchContext.current
    const normalized = normalize(target)
    try {
      const result = await client.fetchQuery({
        queryKey: ['suggest', normalized, group, 'headword'],
        queryFn: ({ signal }) => api<SuggestionResponse>(
          `/suggest?q=${encodeURIComponent(normalized)}&group=${encodeURIComponent(group)}&mode=headword&limit=30`,
          { signal },
        ),
        staleTime: 30_000,
      })
      if (!current()) return
      const exact = result.items.find(item => item.match_kind === 'exact' && normalize(item.word) === normalized &&
        item.dictionary_ids.some(id => scopedDictionaries.some(dictionary => dictionary.id === id)))
      if (exact) openWord(exact.word)
      else {
        setMode('headword')
        setQuery(target)
        beginSearch()
      }
    } catch (error) {
      if (current()) toast.error(error instanceof Error ? error.message : '搜索失败')
    }
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
  const historyItems = bootstrap.data?.history ?? []
  const showHistory = !query.trim()
  // Background refreshes retain the matching candidates, including cached empty results.
  const retainedResults = suggestions.isPlaceholderData && suggestions.data?.retained === true
  const queryPending = composing || normalize(query) !== suggestionQuery || suggestions.isPending || retainedResults
  const suggestionError = suggestions.data ? null : suggestions.error
  const resultItems = suggestions.data?.items ?? []
  const selectedValue = showHistory
    ? (historyItems.some(item => `history-${item.id}` === candidateValue) ? candidateValue : historyItems[0] ? `history-${historyItems[0].id}` : '')
    : (resultItems.some(item => item.word === candidateValue) ? candidateValue : resultItems[0]?.word ?? '')
  useLayoutEffect(() => setCandidateValue(''), [query, group, mode])
  async function submitSearch() {
    if (composing) return
    if (showHistory) {
      const item = historyItems.find(item => `history-${item.id}` === selectedValue) ?? historyItems[0]
      if (item) await openHistoryWord(item.word)
      return
    }
    const context = searchContext.current
    const term = normalize(query)
    if (!term || pendingSubmission.current === context) return
    pendingSubmission.current = context
    try {
      const pending = queryPending || suggestions.isPlaceholderData
      const result = pending ? await client.fetchQuery({
        queryKey: ['suggest', term, group, mode],
        queryFn: ({ signal }) => api<SuggestionResponse>(`/suggest?q=${encodeURIComponent(term)}&group=${encodeURIComponent(group)}&mode=${mode}&limit=30`, { signal }),
        staleTime: 30_000,
      }) : suggestions.data
      if (context !== searchContext.current) return
      const item = (!pending && result?.items.find(item => item.word === selectedValue)) || result?.items[0]
      if (item) openWord(item.word)
    } catch (error) {
      if (context === searchContext.current) toast.error(error instanceof Error ? error.message : '搜索失败')
    } finally {
      if (pendingSubmission.current === context) pendingSubmission.current = null
    }
  }
  const waitingForSuggestions = queryPending || suggestions.isPlaceholderData
  const candidateScope = JSON.stringify([group, mode])
  const heldHeight = !showHistory && waitingForSuggestions && candidateHeight.current?.scope === candidateScope
    ? candidateHeight.current.height : undefined
  useLayoutEffect(() => {
    if (!active || !visible) { candidateHeight.current = null; return }
    if (!showHistory && waitingForSuggestions) return
    const viewport = candidateArea.current?.querySelector<HTMLElement>('.candidate-list')
    if (!viewport) return
    const measure = () => { candidateHeight.current = { scope: candidateScope, height: viewport.getBoundingClientRect().height } }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(viewport)
    return () => observer.disconnect()
  }, [active, visible, showHistory, waitingForSuggestions, candidateScope])
  const [showSuggestionLoading, setShowSuggestionLoading] = useState(false)
  useLayoutEffect(() => {
    setShowSuggestionLoading(false)
    if (!queryPending || composing || !active || showHistory) return
    // Fast local lookups should finish without flashing a loading skeleton on every keystroke.
    const timer = window.setTimeout(() => setShowSuggestionLoading(true), 150)
    return () => window.clearTimeout(timer)
  }, [queryPending, composing, active, showHistory, suggestionQuery, group, mode])

  return (
    <div
      className={cn(
        'dictionary-app',
        isEntry && 'entry-view',
        appearance?.reading_mode && 'reading-mode',
      )}
    >


      <div className="search-space" aria-hidden />
      <motion.section
        ref={searchBox}
        className="search-positioner"
        data-active={active}
        data-entry={isEntry}
        data-animated={animated}
        style={{ maxWidth: isEntry && !appearance?.reading_mode ? '100%' : appearance?.content_width ?? 840 }}
        initial={false}
        animate={{ y: active || isEntry ? offsets.dock : offsets.home }}
        transition={{ duration: animated ? 0.24 : 0, ease: [0.22, 1, 0.36, 1] }}
        aria-label="查词"
      >
        <Command
          shouldFilter={false}
          value={selectedValue}
          onValueChange={setCandidateValue}
          loop
          className="search-command h-auto overflow-visible rounded-none bg-transparent"
          onKeyDownCapture={(event) => {
            if (event.target !== input.current || event.key !== 'Enter') return
            if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 || composing) {
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
                    <Button variant="ghost" size="icon" aria-label="返回上一个词条" className="search-back search-menu-trigger"
                      disabled={!previousWord} onClick={goToPreviousWord}>
                      <ArrowLeft className="size-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">返回上一个词条</TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button variant="ghost" size="icon" aria-label="返回首页" className="search-home search-menu-trigger"
                      onClick={goHome}>
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
              aria-expanded={active}
              placeholder={mode === 'fulltext' ? '在词典正文中搜索' : undefined}
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
              onCompositionEnd={() => {
                setComposing(false)
              }}
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
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="search-group search-menu-trigger" aria-label="当前查询分组"
                  onKeyDown={(event) => event.stopPropagation()}>
                  <span className="truncate">{group === 'all' ? '全部词典' : selectedGroup?.name ?? '分组不可用'}</span>
                  <ChevronDown className="size-3" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" onKeyDown={(event) => event.stopPropagation()}>
                <DropdownMenuItem onSelect={() => changeGroup('all')}>
                  全部词典{group === 'all' && <Check className="ml-auto" />}
                </DropdownMenuItem>
                {bootstrap.data?.groups.map((item) => (
                  <DropdownMenuItem key={item.id} onSelect={() => changeGroup(item.id)}>
                    {item.name}{group === item.id && <Check className="ml-auto" />}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            {isEntry && (            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="词条操作">
                  <MoreHorizontal className="size-5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-48">
                <DropdownMenuItem onClick={() => void toggleReading()}>
                  <BookOpenText />
                  {appearance?.reading_mode ? '退出阅读模式' : '阅读模式'}
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
            </DropdownMenu>)}
          </div>
          <div
            className="suggestion-panel"
            hidden={!active}
            inert={!active}
          >
            <div className="suggestion-toolbar">
              <span className="flex items-center gap-2">
                {showHistory ? (
                  <>
                    <Clock3 className="size-3.5" />
                    最近查词
                  </>
                ) : (
                  <>{mode === 'fulltext' ? '全文匹配' : '候选词条'}</>
                )}
              </span>
              <div className="flex items-center gap-1">
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="sm" className="search-menu-trigger h-8 gap-1.5 text-xs"
                      onKeyDown={(event) => event.stopPropagation()}>
                      <ListFilter className="size-3.5" />
                      {mode === 'headword' ? '词头' : '全文'}
                      <ChevronDown className="size-3" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" onKeyDown={(event) => event.stopPropagation()}>
                    <DropdownMenuItem
                      onClick={() => {
                        setMode('headword')
                      }}
                    >
                      词头匹配{mode === 'headword' && <Check className="ml-auto" />}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onClick={() => {
                        setMode('fulltext')
                      }}
                    >
                      全文检索{mode === 'fulltext' && <Check className="ml-auto" />}
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem asChild>
                      <Link to="/settings/dictionaries?tab=indexes">管理全文索引</Link>
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-8"
                  aria-label="收起搜索"
                  onClick={() => {
                    setActive(false)
                    input.current?.blur()
                  }}
                >
                  <X className="size-4" />
                </Button>
              </div>
            </div>
            <ScrollArea ref={candidateArea} viewportClassName="candidate-list"
              style={{ '--candidate-held-height': heldHeight === undefined ? undefined : `${heldHeight}px` } as CSSProperties}>
            <CommandList className="max-h-none overflow-visible" aria-label={showHistory ? '最近查词' : '候选词条'}
              aria-busy={!showHistory && (queryPending || suggestions.isPlaceholderData)}>
              {showHistory &&
                historyItems.map((item) => (
                  <CommandItem
                    key={item.id}
                    value={`history-${item.id}`}
                    className="candidate"
                    onSelect={() => void openHistoryWord(item.word)}
                  >
                    <Clock3 className="size-4 text-muted-foreground" />
                    <span className="candidate-word grow truncate">{item.word}</span>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-8 text-muted-foreground"
                      aria-label={`删除历史 ${item.word}`}
                      onClick={(event) => {
                        event.preventDefault()
                        event.stopPropagation()
                        void api(`/history/${item.id}`, { method: 'DELETE' })
                          .then(() => client.invalidateQueries({ queryKey: ['bootstrap'] }))
                          .catch((error) => toast.error(error.message))
                      }}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </CommandItem>
                ))}
              {showHistory && historyItems.length === 0 && (
                <div className="candidate-message">
                  {bootstrap.data?.settings.application.history_enabled === false
                    ? '查词历史已关闭'
                    : '暂无查词历史'}
                </div>
              )}
              {!showHistory && queryPending && resultItems.length === 0 && (
                <div className="space-y-3 px-4 py-5 min-h-24" role="status" aria-label={showSuggestionLoading ? '正在搜索' : undefined}>
                  {showSuggestionLoading && <>
                    <Skeleton className="h-4 w-2/3" />
                    <Skeleton className="h-4 w-1/2" />
                  </>}
                </div>
              )}
              {!showHistory && !queryPending && suggestionError && (
                <div className="candidate-message text-destructive" role="alert">
                  {suggestionError.message}
                  <Button variant="ghost" size="sm" onClick={() => void suggestions.refetch()}>
                    重试
                  </Button>
                </div>
              )}
              {!showHistory &&
                !suggestionError &&
                resultItems.map((item) => (
                  <CommandItem
                    key={item.word}
                    value={item.word}
                    className="candidate data-[disabled=true]:opacity-100"
                    disabled={queryPending}
                    onSelect={() => { if (!queryPending) openWord(item.word) }}
                  >
                    <div className="min-w-0 grow">
                      <div className="candidate-word">{item.word}</div>
                      {item.preview && (
                        <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                          {item.preview}
                        </p>
                      )}
                    </div>
                    {item.match_kind === 'fuzzy' && <span className="candidate-count">近似</span>}
                    <ArrowRight className="size-3.5 candidate-arrow" />
                  </CommandItem>
                ))}
              {!showHistory &&
                !queryPending &&
                !suggestionError &&
                suggestionQuery &&
                resultItems.length === 0 && (
                  <div className="candidate-message" role="status">无结果</div>
                )}
            </CommandList>
            </ScrollArea>
          </div>
        </Command>
        {!active && !isEntry && (bootstrap.error || (!bootstrap.isPending && allDictionaries.length === 0)) && (
          <div className="home-status">
            {bootstrap.error ? (
              <Link to="/settings/security" className="text-destructive underline">连接词典服务</Link>
            ) : <Link to="/settings/dictionaries" className="underline">添加词典</Link>}
          </div>
        )}
      </motion.section>
      {isEntry && (
        <main
          className="entry-content"
          style={{ maxWidth: appearance?.reading_mode ? appearance.content_width : undefined }}
        >
          <h1 className="sr-only">{word}</h1>
          {(bootstrap.isPending || hits.isPending) && !bootstrap.error && (
            <div className="space-y-5 py-12" role="status" aria-label="正在查询词典">
              <Skeleton className="h-5 w-40" />
              <Skeleton className="h-4 w-4/5" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-3/5" />
            </div>
          )}
          {(bootstrap.error || hits.error) && (
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
              <Button variant="outline" asChild><Link to="/settings/security">连接设置</Link></Button>
            </div>
          )}
          {hits.isSuccess && dictionaries.length === 0 && (
            <div className="reading-message" role="status">无结果</div>
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
