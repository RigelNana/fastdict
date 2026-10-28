import { useEffect, useState } from 'react'
import { Link, useLocation } from 'react-router-dom'
import { BookOpenText, Menu, PanelLeftClose, PanelLeftOpen, Search, Settings2, X, type LucideIcon } from 'lucide-react'
import { Dialog as DialogPrimitive } from 'radix-ui'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { settingsCategories } from '@/lib/settings-navigation'

function NavigationLink({ icon: Icon, label, to, active, collapsed, onNavigate, preserveSearch, replace }: {
  icon: LucideIcon; label: string; to: string; active: boolean; collapsed: boolean
  onNavigate?: () => void; preserveSearch?: boolean; replace?: boolean
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Link to={to} replace={replace} state={preserveSearch ? { preserve_search: true } : undefined}
          className="workspace-navigation-link" aria-label={label} aria-current={active ? 'page' : undefined}
          onClick={onNavigate}>
          <Icon aria-hidden className="size-[18px] shrink-0" />
          <span className="workspace-navigation-label">{label}</span>
        </Link>
      </TooltipTrigger>
      {collapsed && <TooltipContent side="right" sideOffset={8}>{label}</TooltipContent>}
    </Tooltip>
  )
}

export default function WorkspaceSidebar({ dictionaryUrl, settingsUrl, settingsOpen, collapsed, onCollapsedChange, animated }: {
  dictionaryUrl: string; settingsUrl: string; settingsOpen: boolean
  collapsed: boolean; onCollapsedChange: (collapsed: boolean) => void
  animated: boolean
}) {
  const location = useLocation()
  const [mobileOpen, setMobileOpen] = useState(false)
  useEffect(() => {
    const desktop = matchMedia('(min-width: 761px)')
    const close = () => { if (desktop.matches) setMobileOpen(false) }
    desktop.addEventListener('change', close)
    return () => desktop.removeEventListener('change', close)
  }, [])
  const category = location.pathname.split('/')[2]
  function navigation(compact: boolean, onNavigate?: () => void) {
    return (
      <nav className="workspace-navigation" aria-label="主导航">
        <NavigationLink icon={Search} label="查词" to={dictionaryUrl} active={!settingsOpen}
          collapsed={compact} preserveSearch onNavigate={onNavigate} />
        <NavigationLink icon={Settings2} label="设置" to={settingsUrl} active={settingsOpen}
          collapsed={compact} onNavigate={onNavigate} />
        {settingsOpen && (
          <div className="workspace-settings-navigation" role="group" aria-label="设置分类">
            {settingsCategories.map((item) => (
              <NavigationLink key={item.id} icon={item.icon} label={item.label} to={`/settings/${item.id}`}
                active={category === item.id || (category === 'indexes' && item.id === 'dictionaries')}
                collapsed={compact} onNavigate={onNavigate} replace />
            ))}
          </div>
        )}
      </nav>
    )
  }
  return (
    <>
      <aside className="workspace-sidebar" aria-label="应用导航" data-collapsed={collapsed}>
        <Link to="/" className="workspace-brand wordmark" aria-label="fastdict 首页">
          <BookOpenText className="size-5 shrink-0" strokeWidth={1.8} />
          <span className="workspace-navigation-label">fastdict</span>
        </Link>
        <ScrollArea className="min-h-0 flex-1" viewportClassName="h-full">{navigation(collapsed)}</ScrollArea>
        <Button variant="ghost" className="workspace-sidebar-toggle" aria-label={collapsed ? '展开导航' : '收起导航'}
          title={collapsed ? '展开导航' : '收起导航'} onClick={() => onCollapsedChange(!collapsed)}>
          {collapsed ? <PanelLeftOpen className="size-[18px]" /> : <PanelLeftClose className="size-[18px]" />}
          <span className="workspace-navigation-label">收起导航</span>
        </Button>
      </aside>
      <DialogPrimitive.Root open={mobileOpen} onOpenChange={setMobileOpen}>
        <DialogPrimitive.Trigger asChild>
          <Button variant="secondary" size="icon" className="workspace-menu-toggle" aria-label="打开导航"><Menu className="size-5" /></Button>
        </DialogPrimitive.Trigger>
        <DialogPrimitive.Portal>
          <DialogPrimitive.Overlay className="workspace-drawer-overlay" data-animated={animated} />
          <DialogPrimitive.Content className="workspace-drawer" aria-describedby={undefined} data-animated={animated}>
            <DialogPrimitive.Title className="sr-only">应用导航</DialogPrimitive.Title>
            <div className="flex items-center justify-between pr-3">
              <Link to="/" className="workspace-brand wordmark" aria-label="fastdict 首页" onClick={() => setMobileOpen(false)}>
                <BookOpenText className="size-5" strokeWidth={1.8} /><span>fastdict</span>
              </Link>
              <DialogPrimitive.Close asChild><Button variant="ghost" size="icon" aria-label="关闭导航"><X className="size-4" /></Button></DialogPrimitive.Close>
            </div>
            <ScrollArea className="min-h-0 flex-1" viewportClassName="h-full">{navigation(false, () => setMobileOpen(false))}</ScrollArea>
          </DialogPrimitive.Content>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>
    </>
  )
}
