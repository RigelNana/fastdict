import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Route, Routes, useLocation } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import ConnectionStatus from '@/components/ConnectionStatus'
import WindowScrollbar from '@/components/WindowScrollbar'
import WorkspaceSidebar from '@/components/WorkspaceSidebar'
import { useBootstrap } from '@/lib/api'
import DictionaryPage from '@/pages/DictionaryPage'
import SettingsPage from '@/pages/SettingsPage'
import darkreaderUrl from 'darkreader/darkreader.js?url'

export function useResolvedTheme(theme: 'light' | 'dark' | 'system' = 'system') {
  const [systemDark, setSystemDark] = useState(
    () => matchMedia('(prefers-color-scheme: dark)').matches,
  )
  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const listener = (event: MediaQueryListEvent) => setSystemDark(event.matches)
    media.addEventListener('change', listener)
    return () => media.removeEventListener('change', listener)
  }, [])
  return theme === 'system' ? (systemDark ? 'dark' : 'light') : theme
}

export default function App() {
  const bootstrap = useBootstrap()
  const client = useQueryClient()
  const location = useLocation()
  const settingsOpen = location.pathname.startsWith('/settings')
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => sessionStorage.getItem('fastdict.sidebar-collapsed') === 'true')
  const dictionaryLocation = useRef(settingsOpen ? { ...location, pathname: '/', search: '', hash: '' } : location)
  const settingsLocation = useRef(settingsOpen ? location : { ...location, pathname: '/settings/app', search: '', hash: '' })
  const settingsVisited = useRef(settingsOpen)
  if (!settingsOpen) dictionaryLocation.current = location
  else {
    settingsLocation.current = location
    settingsVisited.current = true
  }
  const dictionaryUrl = dictionaryLocation.current.pathname + dictionaryLocation.current.search
  const paneScroll = useRef({ dictionary: 0, settings: 0 })
  const activePane = useRef(settingsOpen)
  activePane.current = settingsOpen
  useLayoutEffect(() => {
    const pane = settingsOpen ? 'settings' : 'dictionary'
    const position = paneScroll.current[pane]
    let restoring = true
    const frame = requestAnimationFrame(() => {
      window.scrollTo({ top: position, behavior: 'instant' })
      restoring = false
    })
    const remember = () => {
      if (!restoring && activePane.current === settingsOpen) paneScroll.current[pane] = window.scrollY
    }
    window.addEventListener('scroll', remember, { passive: true })
    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener('scroll', remember)
    }
  }, [settingsOpen])
  useEffect(() => {
    if (settingsOpen) document.title = '设置 · fastdict'
    else {
      const word = dictionaryLocation.current.pathname.match(/^\/word\/(.+)$/)?.[1]
      document.title = word ? `${decodeURIComponent(word)} · fastdict` : 'fastdict · 查词'
    }
  }, [settingsOpen])
  const theme = useResolvedTheme(bootstrap.data?.settings.appearance.theme)
  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark')
    // Start the shared runtime alongside lookup, before any article iframe exists.
    if (theme === 'dark' && !document.getElementById('article-theme-preload')) {
      const preload = document.createElement('link')
      preload.id = 'article-theme-preload'
      preload.rel = 'preload'
      preload.as = 'script'
      preload.href = darkreaderUrl
      document.head.append(preload)
    }
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute('content', theme === 'dark' ? '#09090b' : '#ffffff')
  }, [theme])
  useEffect(() => {
    const reset = () => {
      void client.cancelQueries().then(() => client.resetQueries())
    }
    window.addEventListener('fastdict:connection', reset)
    return () => window.removeEventListener('fastdict:connection', reset)
  }, [client])
  return (
    <TooltipProvider>
      <div className="workspace-shell" data-sidebar-collapsed={sidebarCollapsed} data-animated={bootstrap.data?.settings.appearance.motion !== false}>
      <WorkspaceSidebar dictionaryUrl={dictionaryUrl} settingsUrl={settingsLocation.current.pathname + settingsLocation.current.search}
        animated={bootstrap.data?.settings.appearance.motion !== false}
        settingsOpen={settingsOpen} collapsed={sidebarCollapsed} onCollapsedChange={(collapsed) => {
          setSidebarCollapsed(collapsed)
          sessionStorage.setItem('fastdict.sidebar-collapsed', String(collapsed))
        }} />
      <ConnectionStatus />
      <div className="workspace-content">
      <div hidden={settingsOpen} inert={settingsOpen}>
        <Routes location={dictionaryLocation.current}>
          <Route path="/*" element={<DictionaryPage theme={theme} visible={!settingsOpen} />} />
        </Routes>
      </div>
      <div hidden={!settingsOpen} inert={!settingsOpen}>
      {settingsVisited.current && (
        <Routes location={settingsLocation.current}>
          <Route path="/settings/*" element={<SettingsPage />} />
        </Routes>
      )}
      </div>
      </div>
      <WindowScrollbar />
      </div>
      <Toaster theme={theme} position="bottom-right" richColors closeButton />
    </TooltipProvider>
  )
}
