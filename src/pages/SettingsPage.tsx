import { useEffect, useState } from 'react'
import { Navigate, useLocation, useSearchParams } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { Monitor, Moon, RotateCcw, Sun } from 'lucide-react'
import { resolveSettingsCategory, settingsCategories as categories } from '@/lib/settings-navigation'
import { api, getConnection, getDesktopBridge, setConnection, useBootstrap } from '@/lib/api'
import { connectionKey } from '@/lib/connection-storage'
import type { Bootstrap, Settings } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { useAutosave } from '@/lib/use-autosave'
import { Input } from '@/components/ui/input'
import { Slider } from '@/components/ui/slider'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import DictionariesManager from '@/components/settings/DictionariesManager'
import { BackupPanel, LogsPanel } from '@/components/settings/MaintenancePanels'
import {
  Choice,
  Confirm,
  ErrorNotice,
  Field,
  NumberField,
  Section,
  SaveStatus,
  Toggle,
  bytes,
  useAction,
} from '@/components/settings/shared'

function ConnectionSettings() {
  const desktop = getDesktopBridge()
  const client = useQueryClient()
  const [connection, update] = useState(getConnection)
  const [status, setStatus] = useState('')
  const { pending, error, run } = useAction()
  useEffect(() => {
    if (desktop?.local_only) return
    const sync = (event: StorageEvent) => {
      if (event.key === connectionKey || event.key === null) update(getConnection())
    }
    window.addEventListener('storage', sync)
    return () => window.removeEventListener('storage', sync)
  }, [desktop?.local_only])
  if (desktop?.local_only) {
    return (
      <Section
        title="本地内置引擎"
        description="桌面版内置本地词典服务，所有词典数据与检索索引均保存在本机，无需配置远程地址或访问令牌。"
      >
        <div className="space-y-1.5 rounded-lg border bg-muted/25 px-4 py-3 text-xs text-muted-foreground">
          <div>
            运行模式：<strong className="font-medium text-emerald-600 dark:text-emerald-400">本地独立引擎运行中</strong>
          </div>
          <div className="break-all">
            数据目录：<code className="rounded bg-muted px-1 py-0.5 text-foreground">{desktop.data_dir}</code>
          </div>
        </div>
      </Section>
    )
  }
  async function connect() {
    const previous = getConnection()
    if (pending || (previous.base_url === connection.base_url && previous.token === connection.token)) return
    setStatus('')
    await run(async () => {
      try {
        setConnection(connection)
        const health = await api<{ ok: boolean; version: string }>('/health')
        await api<Bootstrap>('/bootstrap')
        setStatus(`已连接 · v${health.version}`)
      } catch (cause) {
        setConnection(previous)
        throw cause
      }
      client.removeQueries({ queryKey: ['logs'] })
      client.removeQueries({ queryKey: ['resource-entries'] })
    }, '')
  }
  return (
    <Section title="服务连接" description="默认连接当前托管地址；连接远程主机时填写根地址与访问令牌。">
      <Field label="服务根地址">
        <Input
          aria-label="服务根地址"
          placeholder="留空使用当前站点服务"
          autoComplete="url"
          value={connection.base_url}
          onChange={(event) => update({ ...connection, base_url: event.target.value })}
          onBlur={() => void connect()}
        />
      </Field>
      <Field label="API 访问令牌">
        <Input
          aria-label="服务访问令牌"
          type="password"
          placeholder="本地回环地址通常留空"
          autoComplete="off"
          value={connection.token}
          onChange={(event) => update({ ...connection, token: event.target.value })}
          onBlur={() => void connect()}
        />
      </Field>
      <ErrorNotice error={error} />
      {status && (
        <p role="status" className="text-sm font-medium text-emerald-600 dark:text-emerald-400">
          {status}
        </p>
      )}
    </Section>
  )
}

const validSettings = (draft: Settings) =>
  Number.isInteger(draft.application.history_limit) &&
  draft.application.history_limit >= 0 &&
  draft.application.history_limit <= 10000 &&
  draft.appearance.font_size >= 12 &&
  draft.appearance.font_size <= 36 &&
  draft.appearance.line_height >= 1 &&
  draft.appearance.line_height <= 3 &&
  draft.appearance.content_width >= 560 &&
  draft.appearance.content_width <= 1100 &&
  draft.spelling.distance >= 0 &&
  draft.spelling.distance <= 2 &&
  Number.isInteger(draft.performance.max_open_readers) &&
  draft.performance.max_open_readers >= 1 &&
  draft.performance.max_open_readers <= 64 &&
  Number.isInteger(draft.performance.cache_mb) &&
  draft.performance.cache_mb >= 1 &&
  draft.performance.cache_mb <= 4096 &&
  Number.isInteger(draft.performance.query_workers) &&
  draft.performance.query_workers >= 1 &&
  draft.performance.query_workers <= 32

function SliderField({
  label,
  value,
  display,
  min,
  max,
  step = 1,
  onChange,
}: {
  label: string
  value: number
  display: string
  min: number
  max: number
  step?: number
  onChange: (value: number) => void
}) {
  return (
    <Field label={label}>
      <div className="flex items-center gap-4">
        <Slider
          aria-label={label}
          min={min}
          max={max}
          step={step}
          value={[value]}
          onValueChange={([next]) => {
            if (typeof next === 'number') onChange(Number(next.toFixed(2)))
          }}
        />
        <span className="w-16 shrink-0 text-right text-sm font-medium tabular-nums text-foreground">
          {display}
        </span>
      </div>
    </Field>
  )
}

function SettingsLoading({ dictionaries }: { dictionaries: boolean }) {
  const [visible, setVisible] = useState(false)
  useEffect(() => {
    const timer = setTimeout(() => setVisible(true), 180)
    return () => clearTimeout(timer)
  }, [])
  return (
    <div
      role="status"
      aria-label="正在读取设置"
      className="settings-loading"
      style={{ visibility: visible ? 'visible' : 'hidden' }}
    >
      <div aria-hidden className={dictionaries ? 'dictionary-manager' : undefined}>
        {dictionaries && (
          <div className="dictionary-list space-y-5">
            <div className="settings-loading-control" />
            <div className="settings-loading-label" />
            <div className="settings-loading-label" />
          </div>
        )}
        <div className={dictionaries ? 'dictionary-detail space-y-7' : 'space-y-7'}>
          {[0, 1].map((section) => (
            <div key={section} className="settings-section">
              <div className="settings-loading-heading" />
              {[0, 1, 2].map((row) => (
                <div key={row} className="settings-field">
                  <div className="settings-loading-label" />
                  <div className="settings-loading-control" />
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

function ConfigurationPanel({ data, category }: { data: Bootstrap; category: string }) {
  const { pending, error, run } = useAction()
  const autosave = useAutosave({
    value: data.settings,
    resource: '/settings',
    valid: validSettings,
    save: (draft) => api<Settings>('/settings', { method: 'PUT', body: JSON.stringify(draft) }),
  })
  const { draft } = autosave

  function patch<K extends keyof Settings>(key: K, value: Partial<Settings[K]>) {
    const immediate =
      Object.values(value).every((v) => typeof v === 'boolean') ||
      ['theme', 'default_group', 'external_links', 'distance'].some((field) => field in value)
    autosave.change((current) => ({ ...current, [key]: { ...current[key], ...value } }), immediate)
  }

  const typographyModified =
    draft.appearance.font_size !== 16 ||
    draft.appearance.line_height !== 1.7 ||
    draft.appearance.content_width !== 840 ||
    draft.appearance.font_family !== ''

  return (
    <div className="settings-form space-y-10" onBlurCapture={() => void autosave.flush()}>
      {category === 'appearance' && (
        <>
          <Section title="界面主题与动效">
            <div className="theme-card-grid" role="radiogroup" aria-label="界面主题">
              {(
                [
                  { value: 'light', label: '浅色模式', icon: Sun },
                  { value: 'dark', label: '深色模式', icon: Moon },
                  { value: 'system', label: '跟随系统', icon: Monitor },
                ] as const
              ).map(({ value, label, icon: Icon }) => {
                const selected = draft.appearance.theme === value
                return (
                  <button
                    key={value}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    className="theme-card"
                    data-selected={selected}
                    onClick={() => patch('appearance', { theme: value })}
                  >
                    <Icon className="size-4" />
                    <span>{label}</span>
                  </button>
                )
              })}
            </div>
            <Toggle
              label="界面过渡动画"
              checked={draft.appearance.motion}
              onChange={(motion) => patch('appearance', { motion })}
            />
            <Toggle
              label="专注阅读模式（居中限制词条最大行宽）"
              checked={draft.appearance.reading_mode}
              onChange={(reading_mode) => patch('appearance', { reading_mode })}
            />
          </Section>

          <Section
            title="全局正文排版"
            description="作用于排版策略设为“应用全局字号与行高”或“强制覆盖”的词典。"
          >
            <SliderField
              label="正文字号"
              value={draft.appearance.font_size}
              display={`${draft.appearance.font_size} px`}
              min={12}
              max={36}
              step={1}
              onChange={(font_size) => patch('appearance', { font_size })}
            />
            <SliderField
              label="正文行高"
              value={draft.appearance.line_height}
              display={draft.appearance.line_height.toFixed(1)}
              min={1.2}
              max={2.6}
              step={0.1}
              onChange={(line_height) => patch('appearance', { line_height })}
            />
            <SliderField
              label="阅读模式宽度"
              value={draft.appearance.content_width}
              display={`${draft.appearance.content_width} px`}
              min={560}
              max={1100}
              step={20}
              onChange={(content_width) => patch('appearance', { content_width })}
            />
            <Field label="默认正文字体">
              <Input
                aria-label="默认正文字体"
                placeholder="留空使用清晰无衬线与中文字体栈"
                value={draft.appearance.font_family}
                onChange={(event) => patch('appearance', { font_family: event.target.value })}
              />
            </Field>
            {typographyModified && (
              <Button
                variant="outline"
                size="sm"
                onClick={() =>
                  autosave.change(
                    (current) => ({
                      ...current,
                      appearance: {
                        ...current.appearance,
                        font_size: 16,
                        line_height: 1.7,
                        content_width: 840,
                        font_family: '',
                      },
                    }),
                    true,
                  )
                }
              >
                <RotateCcw className="size-3.5" />
                恢复默认排版
              </Button>
            )}
            <div
              className="rounded-xl border bg-muted/25 px-5 py-4"
              style={{
                fontSize: draft.appearance.font_size,
                lineHeight: draft.appearance.line_height,
                fontFamily: draft.appearance.font_family || 'inherit',
              }}
            >
              <div className="font-semibold">harness <span className="font-normal text-muted-foreground">/ˈhɑːrnɪs/</span></div>
              <div className="mt-1">① 挽具，马具；背带，安全带。② 控制并利用（自然力或资源）。</div>
            </div>
          </Section>
        </>
      )}

      {category === 'preferences' && (
        <>
          <Section title="查询范围">
            <Field label="默认查询分组">
              <Choice
                label="默认查询分组"
                value={draft.application.default_group || 'all'}
                items={[
                  { value: 'all', label: '全部启用词典' },
                  ...data.groups.map((group) => ({ value: group.id, label: group.name })),
                ]}
                onChange={(default_group) => patch('application', { default_group })}
              />
            </Field>
          </Section>

          <Section title="拼写纠错与近似提示" description="当输入词头无精确或前缀匹配时，在候选列表中提示拼写相近的词条。">
            <Toggle
              label="启用拼写近似建议"
              checked={draft.spelling.enabled}
              onChange={(enabled) => patch('spelling', { enabled })}
            />
            {draft.spelling.enabled && (
              <Field label="容错编辑距离">
                <div className="segmented-control" role="radiogroup" aria-label="允许字符差异">
                  {[
                    { value: 0, label: '精确' },
                    { value: 1, label: '1 字符' },
                    { value: 2, label: '2 字符' },
                  ].map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      role="radio"
                      aria-checked={draft.spelling.distance === option.value}
                      className="segmented-item"
                      data-active={draft.spelling.distance === option.value}
                      onClick={() => patch('spelling', { distance: option.value })}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </Field>
            )}
          </Section>

          <Section title="查词记录保留">
            <Toggle
              label="服务端记录查词历史"
              checked={draft.application.history_enabled}
              onChange={(history_enabled) => patch('application', { history_enabled })}
            />
            <NumberField
              label="记录条数上限（0–10000）"
              value={draft.application.history_limit}
              min={0}
              max={10000}
              onChange={(history_limit) => patch('application', { history_limit })}
            />
            <Confirm
              title="清空全部查询记录？"
              description="已保存的记录将永久删除，不可撤销。"
              label={`清空记录（当前 ${data.history.length} 条）`}
              destructive
              disabled={pending || !data.history.length}
              onConfirm={() => run(() => api('/history', { method: 'DELETE' }), '查询记录已清空')}
            />
          </Section>
        </>
      )}

      {category === 'system' && (
        <>
          <Section title="词典内容安全">
            <Field label="词条内外部网页链接">
              <Choice
                label="外部链接策略"
                value={draft.security.external_links}
                items={[
                  { value: 'ask', label: '打开前弹窗确认' },
                  { value: 'allow', label: '直接在新标签页打开' },
                  { value: 'block', label: '始终阻止' },
                ]}
                onChange={(external_links) =>
                  patch('security', {
                    external_links: external_links as Settings['security']['external_links'],
                  })
                }
              />
            </Field>
          </Section>

          <Section title="查询引擎与索引维护">
            <div className="space-y-3 rounded-lg border bg-muted/25 px-4 py-3.5 text-xs text-muted-foreground">
              <div className="flex flex-wrap gap-x-5 gap-y-1">
                <span>目录词头：<strong className="font-medium text-foreground">{data.stats.words.toLocaleString()}</strong></span>
                <span>常驻句柄：<strong className="font-medium text-foreground">{data.stats.open_readers} 部</strong></span>
                <span>块缓存占用：<strong className="font-medium text-foreground">{bytes(data.stats.cache_bytes)}</strong></span>
                <span>
                  全文索引：
                  <strong className="font-medium text-foreground">
                    {data.dictionaries.filter((d) => d.index_status.state === 'ready').length} / {data.dictionaries.length} 部就绪
                  </strong>
                </span>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {data.dictionaries.length > 0 && (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={pending || data.dictionaries.some((d) => d.index_status.state === 'building')}
                    onClick={() =>
                      void run(
                        () => api('/maintenance', { method: 'POST', body: JSON.stringify({ target: 'fulltext', action: 'rebuild' }) }),
                        '已提交全部词典全文索引更新任务',
                      )
                    }
                  >
                    {data.dictionaries.some((d) => d.index_status.state === 'building') ? '正在构建全文索引…' : '更新全部全文索引'}
                  </Button>
                )}
                {data.dictionaries.length > 0 && (
                  <Confirm
                    title="重建全部词头目录？"
                    description="从已注册词典重新提取词头并更新检索目录，不会删除词典或全文索引。"
                    label="重建词头目录"
                    disabled={pending}
                    onConfirm={() =>
                      run(
                        () => api('/maintenance', { method: 'POST', body: JSON.stringify({ target: 'headwords', action: 'rebuild' }) }),
                        '词头目录重建任务已提交',
                      )
                    }
                  />
                )}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={pending}
                  onClick={() =>
                    void run(
                      () => api('/maintenance', { method: 'POST', body: JSON.stringify({ target: 'cache', action: 'clear' }) }),
                      '已释放内存块缓存',
                    )
                  }
                >
                  释放缓存
                </Button>
              </div>
            </div>
            <SliderField
              label="常驻打开词典上限"
              value={draft.performance.max_open_readers}
              display={`${draft.performance.max_open_readers} 部`}
              min={1}
              max={32}
              step={1}
              onChange={(max_open_readers) => patch('performance', { max_open_readers })}
            />
            <SliderField
              label="解压块缓存上限"
              value={draft.performance.cache_mb}
              display={`${draft.performance.cache_mb} MB`}
              min={16}
              max={1024}
              step={16}
              onChange={(cache_mb) => patch('performance', { cache_mb })}
            />
            <SliderField
              label="并发查询线程数"
              value={draft.performance.query_workers}
              display={`${draft.performance.query_workers} 线程`}
              min={1}
              max={16}
              step={1}
              onChange={(query_workers) => patch('performance', { query_workers })}
            />
          </Section>
        </>
      )}

      <ErrorNotice error={error} />
      {!autosave.valid && <p role="alert" className="text-sm text-destructive">请检查数值范围是否合法。</p>}
      <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
    </div>
  )
}

export default function SettingsPage() {
  const data = useBootstrap()
  const location = useLocation()
  const [search, setSearch] = useSearchParams()
  const rawSegment = location.pathname.split('/')[2]

  if (rawSegment === 'indexes' || search.get('tab') === 'indexes') return <Navigate to="/settings/dictionaries" replace />
  if (rawSegment === 'app' || rawSegment === 'spelling') return <Navigate to="/settings/preferences" replace />
  if (rawSegment === 'security' || rawSegment === 'performance' || rawSegment === 'backup' || rawSegment === 'logs') {
    return <Navigate to="/settings/system" replace />
  }

  const resolvedId = resolveSettingsCategory(rawSegment)
  const category = categories.find((item) => item.id === resolvedId) ?? categories[0]

  return (
    <div className="settings-layout">
      <main
        id="settings-content"
        aria-busy={data.isPending && !data.data}
        className={category.id === 'dictionaries' ? 'settings-content settings-content-wide' : 'settings-content'}
      >
        <h2 className="settings-title">{category.label}</h2>
        {category.id === 'system' && (
          <div className="mb-8">
            <ConnectionSettings />
          </div>
        )}
        {data.isPending && !data.data ? (
          <SettingsLoading dictionaries={category.id === 'dictionaries'} />
        ) : (
          data.data && (
            <>
              {category.id === 'dictionaries' && (
                <DictionariesManager
                  data={data.data}
                  tab={search.get('tab') === 'groups' ? 'groups' : 'manage'}
                  onTabChange={(tab) => {
                    const next = new URLSearchParams(search)
                    next.set('tab', tab)
                    setSearch(next, { replace: true })
                  }}
                  selected={search.get('dictionary')}
                  onSelect={(id) => {
                    const next = new URLSearchParams(search)
                    if (id) next.set('dictionary', id)
                    else next.delete('dictionary')
                    setSearch(next, { replace: true })
                  }}
                />
              )}
              <div hidden={category.id === 'dictionaries'}>
                <ConfigurationPanel key={getConnection().base_url} data={data.data} category={category.id} />
              </div>
              {category.id === 'system' && (
                <div className="mt-10 space-y-10 border-t pt-8">
                  <BackupPanel />
                  <Accordion type="single" collapsible className="rounded-xl border px-5">
                    <AccordionItem value="logs" className="border-0">
                      <AccordionTrigger className="settings-section-title py-4 hover:no-underline">
                        服务诊断日志
                      </AccordionTrigger>
                      <AccordionContent className="pb-4">
                        <LogsPanel />
                      </AccordionContent>
                    </AccordionItem>
                  </Accordion>
                </div>
              )}
            </>
          )
        )}
      </main>
    </div>
  )
}
