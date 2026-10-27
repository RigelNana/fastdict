import { useState } from 'react'
import { Navigate, useLocation, useSearchParams } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { settingsCategories as categories } from '@/lib/settings-navigation'
import { api, getConnection, setConnection, useBootstrap } from '@/lib/api'
import type { Bootstrap, Settings } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { useAutosave } from '@/lib/use-autosave'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
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
  useAction,
} from '@/components/settings/shared'

function ConnectionSettings() {
  const client = useQueryClient()
  const [connection, update] = useState(getConnection)
  const [status, setStatus] = useState('')
  const { pending, error, run } = useAction()
  async function connect() {
    const previous = getConnection()
    if (pending || (previous.base_url === connection.base_url && previous.token === connection.token)) return
    setStatus('')
    await run(async () => {
      try {
        setConnection(connection)
        const health = await api<{ ok: boolean; version: string }>('/health')
        await api<Bootstrap>('/bootstrap')
        setStatus(`已连接 · ${health.version}`)
      } catch (cause) {
        setConnection(previous)
        throw cause
      }
      client.removeQueries({ queryKey: ['logs'] })
      client.removeQueries({ queryKey: ['resource-entries'] })
    }, '')
  }
  return (
    <Section title="服务连接">
      <Field label="服务地址">
        <Input
          aria-label="服务根地址"
          placeholder="留空使用当前服务"
          autoComplete="url"
          value={connection.base_url}
          onChange={(event) => update({ ...connection, base_url: event.target.value })}
          onBlur={() => void connect()}
        />
      </Field>
      <Field label="访问令牌">
        <Input
          aria-label="服务访问令牌"
          type="password"
          autoComplete="off"
          value={connection.token}
          onChange={(event) => update({ ...connection, token: event.target.value })}
          onBlur={() => void connect()}
        />
      </Field>
      <ErrorNotice error={error} />
      {status && (
        <p role="status" className="text-sm text-primary">
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
    const immediate = Object.values(value).every((v) => typeof v === 'boolean') ||
      ['theme', 'default_group', 'external_links', 'distance'].some((field) => field in value)
    autosave.change((current) => ({ ...current, [key]: { ...current[key], ...value } }), immediate)
  }

  return (
    <div className="settings-form space-y-10" onBlurCapture={() => void autosave.flush()}>
      {category === 'app' && (
        <>
          <Section title="默认查询">
            <Field label="默认分组">
              <Choice
                label="默认查询分组"
                value={draft.application.default_group || 'all'}
                items={[
                  { value: 'all', label: '全部词典' },
                  ...data.groups.map((group) => ({ value: group.id, label: group.name })),
                ]}
                onChange={(default_group) => patch('application', { default_group })}
              />
            </Field>
          </Section>
          <Section title="查询历史">
            <Toggle
              label="保留查询历史"
              checked={draft.application.history_enabled}
              onChange={(history_enabled) => patch('application', { history_enabled })}
            />
            <NumberField
              label="历史上限（0–10000）"
              value={draft.application.history_limit}
              min={0}
              max={10000}
              onChange={(history_limit) => patch('application', { history_limit })}
            />
            <Confirm
              title="清空全部查询历史？"
              description="查询记录将永久删除。"
              label="清空历史"
              destructive
              disabled={pending || !data.history.length}
              onConfirm={() => run(() => api('/history', { method: 'DELETE' }), '查询历史已清空')}
            />
          </Section>
        </>
      )}
      {category === 'appearance' && (
        <>
          <Section title="界面与阅读">
            <Field label="主题">
              <Choice
                label="界面主题"
                value={draft.appearance.theme}
                items={[
                  { value: 'system', label: '跟随系统' },
                  { value: 'light', label: '浅色' },
                  { value: 'dark', label: '深色' },
                ]}
                onChange={(theme) =>
                  patch('appearance', { theme: theme as Settings['appearance']['theme'] })
                }
              />
            </Field>
            <Toggle
              label="界面动画"
              checked={draft.appearance.motion}
              onChange={(motion) => patch('appearance', { motion })}
            />
            <Toggle
              label="阅读模式"
              checked={draft.appearance.reading_mode}
              onChange={(reading_mode) => patch('appearance', { reading_mode })}
            />
          </Section>
          <Section title="正文排版">
            <NumberField
              label="字号（px）"
              value={draft.appearance.font_size}
              min={12}
              max={36}
              onChange={(font_size) => patch('appearance', { font_size })}
            />
            <NumberField
              label="行高"
              value={draft.appearance.line_height}
              min={1}
              max={3}
              step={0.1}
              onChange={(line_height) => patch('appearance', { line_height })}
            />
            <NumberField
              label="阅读模式宽度（px）"
              value={draft.appearance.content_width}
              min={560}
              max={1100}
              step={20}
              onChange={(content_width) => patch('appearance', { content_width })}
            />
            <Field
              label="默认正文字体"
            >
              <Input
                aria-label="默认正文字体"
                placeholder="使用词典字体"
                value={draft.appearance.font_family}
                onChange={(event) => patch('appearance', { font_family: event.target.value })}
              />
            </Field>
            <div
              className="border-y py-5"
              style={{
                fontSize: draft.appearance.font_size,
                lineHeight: draft.appearance.line_height,
                fontFamily: draft.appearance.font_family || 'inherit',
              }}
            >
              阅读从一个词开始。
              <br />
              Every word opens a world.
            </div>
          </Section>
        </>
      )}
      {category === 'security' && (
        <>
          <Section title="词典内容安全">
            <Field
              label="打开外部链接"
            >
              <Choice
                label="外部链接策略"
                value={draft.security.external_links}
                items={[
                  { value: 'ask', label: '每次询问' },
                  { value: 'allow', label: '允许' },
                  { value: 'block', label: '阻止' },
                ]}
                onChange={(external_links) =>
                  patch('security', {
                    external_links: external_links as Settings['security']['external_links'],
                  })
                }
              />
            </Field>

          </Section>
        </>
      )}
      {category === 'spelling' && (
        <Section
          title="拼写建议"
        >
          <Toggle
            label="启用拼写建议"
            checked={draft.spelling.enabled}
            onChange={(enabled) => patch('spelling', { enabled })}
          />
          <Field
            label="允许字符差异"
          >
            <Choice
              label="允许字符差异"
              value={String(draft.spelling.distance)}
              items={[
                { value: '0', label: '0 · 不接受差异' },
                { value: '1', label: '1 个字符' },
                { value: '2', label: '2 个字符' },
              ]}
              onChange={(distance) => patch('spelling', { distance: Number(distance) })}
            />
          </Field>
        </Section>
      )}
      {category === 'performance' && (
        <>
          <Section
            title="查询性能"
          >
            <NumberField
              label="最多打开词典数"
              value={draft.performance.max_open_readers}
              min={1}
              max={64}
              onChange={(max_open_readers) => patch('performance', { max_open_readers })}
            />
            <NumberField
              label="缓存上限（MB）"
              value={draft.performance.cache_mb}
              min={1}
              max={4096}
              onChange={(cache_mb) => patch('performance', { cache_mb })}
            />
            <NumberField
              label="查询线程"
              value={draft.performance.query_workers}
              min={1}
              max={32}
              onChange={(query_workers) => patch('performance', { query_workers })}
            />
          </Section>
        </>
      )}
      <ErrorNotice error={error} />
      {!autosave.valid && <p role="alert" className="text-sm text-destructive">请检查数值范围。</p>}
      <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
    </div>
  )
}
export default function SettingsPage() {
  const data = useBootstrap()
  const location = useLocation()
  const [search, setSearch] = useSearchParams()
  const category = categories.find((item) => item.id === location.pathname.split('/')[2]) ?? categories[0]
  if (location.pathname === '/settings/indexes') return <Navigate to="/settings/dictionaries?tab=indexes" replace />
  return (
    <div className="settings-layout">
      <main id="settings-content" className={category.id === 'dictionaries' ? 'settings-content settings-content-wide' : 'settings-content'}>
        <h2 className="settings-title">{category.label}</h2>
        {category.id === 'security' && <div className="mb-7"><ConnectionSettings /></div>}
        {data.isPending && !data.data ? (
          <div className="space-y-5" role="status" aria-label="正在读取设置">
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-12 w-full" />
            <Skeleton className="h-12 w-full" />
          </div>
        ) : data.data && (
          <>
            {category.id === 'dictionaries' && (
              <DictionariesManager
                data={data.data}
                tab={['manage', 'groups', 'indexes'].includes(search.get('tab') ?? '') ? search.get('tab')! : 'manage'}
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
            {category.id === 'backup' && <BackupPanel />}
            {category.id === 'logs' && <LogsPanel />}
            <div hidden={['dictionaries', 'backup', 'logs'].includes(category.id)}>
              <ConfigurationPanel key={getConnection().base_url} data={data.data} category={category.id} />
            </div>
          </>
        )}
      </main>
    </div>
  )
}
