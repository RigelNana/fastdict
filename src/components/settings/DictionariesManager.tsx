import { useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Plus, ChevronDown, Upload, FolderSearch, Server, RefreshCw, Trash2, LoaderCircle } from 'lucide-react'
import { toast } from 'sonner'
import { api, apiUpload, type UploadProgress } from '@/lib/api'
import { useAutosave } from '@/lib/use-autosave'
import type { Bootstrap, Dictionary, Resource } from '@/lib/types'
import { DictionaryIcon } from '@/components/EntrySection'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import {
  Choice,
  Confirm,
  Empty,
  ErrorNotice,
  Field,
  Section,
  SaveStatus,
  Toggle,
  useAction,
} from './shared'
import ImportDictionaries, { type ImportHandle } from './ImportDictionaries'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import GroupsManager from './GroupsManager'
import ResourceBrowser from './ResourceBrowser'
import ResourceManager from './ResourceManager'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Badge } from '@/components/ui/badge'
import IndexBadge from './IndexBadge'
import OperationProgress from './OperationProgress'

const editableDictionary = (dictionary: Dictionary) => ({
  name: dictionary.name.trim(),
  enabled: dictionary.enabled,
  icon: dictionary.icon || null,
  custom_css: dictionary.custom_css,
  font_family: dictionary.font_family,
  typography_mode: dictionary.typography_mode,
  resources: dictionary.resources.map((resource, order) => ({ ...resource, order })),
})

function DictionaryDetails({
  dictionary,
  back,
}: {
  dictionary: Dictionary
  back: () => void
}) {
  const [manualIcon, setManualIcon] = useState(false)
  const [browsing, setBrowsing] = useState<Resource | null>(null)
  const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(null)
  const [uploadDetail, setUploadDetail] = useState('')
  const { pending, error, run } = useAction()
  const base = `/dictionaries/${encodeURIComponent(dictionary.id)}`
  const autosave = useAutosave({
    value: dictionary,
    resource: base,
    project: editableDictionary,
    valid: (value) => !!value.name.trim() && value.resources.every((item) => item.kind === 'mdd' || !!item.key.trim()),
    save: (value) => {
      const { resources, ...fields } = editableDictionary(value)
      const changed = JSON.stringify(resources) !== JSON.stringify(editableDictionary(dictionary).resources)
      return api<Dictionary>(base, { method: 'PUT', body: JSON.stringify(changed ? { ...fields, resources } : fields) })
    },
  })
  const { draft, change: setDraft } = autosave

  async function save(patch: Partial<Dictionary>) {
    if (!(await autosave.flush())) return
    return run(() => api<Dictionary>(base, { method: 'PUT', body: JSON.stringify(patch) }))
  }

  async function upload(files: FileList | null) {
    const selected = Array.from(files ?? [])
    if (!selected.length || pending) return
    const replacing = selected.some((file) =>
      dictionary.resources.some((resource) => resource.path === file.name),
    )
    setUploadDetail(selected.length === 1 ? selected[0].name : `${selected.length} 个文件`)
    setUploadProgress(null)
    const body = new FormData()
    for (const file of selected) body.append('files', file, file.name)
    const result = await run(
      async () => {
        if (!(await autosave.flush())) throw new Error('请先处理未保存的修改。')
        const updated = await apiUpload<Dictionary>(`${base}/resources`, body, setUploadProgress)
        autosave.replace(updated)
        return updated
      },
      replacing ? '资源已更新' : '资源已添加',
    )
    if (result) autosave.replace(result)
    setUploadDetail('')
    setUploadProgress(null)
  }

  const metadata = dictionary.metadata
  const imageOptions = draft.resources
    .filter((resource) => resource.kind === 'image' && resource.enabled && resource.key)
    .filter(
      (resource, index, resources) =>
        resources.findIndex((other) => other.key === resource.key) === index,
    )
    .map((resource) => ({ value: resource.key, label: resource.path }))
  const customIcon = !!draft.icon && !imageOptions.some((option) => option.value === draft.icon)
  const indexBusy = dictionary.index_status.state === 'building'
  const indexPresent = dictionary.index_status.state === 'ready' || dictionary.index_status.state === 'error'

  return (
    <div className="space-y-6">
      <div className="rounded-xl border bg-muted/25 p-4 sm:p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="flex min-w-0 flex-1 items-start gap-3.5">
            <div className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg border bg-background">
              <DictionaryIcon dictionary={dictionary} />
            </div>
            <div className="min-w-0 flex-1">
              <h3
                className="min-w-0 line-clamp-2 [overflow-wrap:anywhere] text-lg font-semibold leading-snug"
                title={dictionary.name}
              >
                {dictionary.name}
              </h3>
              <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <span className="tabular-nums">{dictionary.entries.toLocaleString()} 词条</span>
                <span>·</span>
                <span>{dictionary.resources.length} 个资源</span>
                <IndexBadge status={dictionary.index_status} />
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={pending || indexBusy}
              onClick={() =>
                void run(
                  () =>
                    api('/maintenance', {
                      method: 'POST',
                      body: JSON.stringify({
                        target: 'fulltext',
                        action: 'rebuild',
                        dictionary_id: dictionary.id,
                      }),
                    }),
                  '已提交全文索引任务',
                )
              }
            >
              <RefreshCw className={indexBusy ? 'size-3.5 animate-spin' : 'size-3.5'} />
              {indexBusy
                ? '索引构建中…'
                : dictionary.index_status.state === 'ready'
                  ? '更新全文索引'
                  : '建立全文索引'}
            </Button>
            {indexPresent && !indexBusy && (
              <Confirm
                label="清除索引"
                title={`清除“${dictionary.name}”的全文索引？`}
                description="仅清除派生的全文检索索引，词头查询与词典本体不受影响。"
                destructive
                disabled={pending}
                onConfirm={() =>
                  run(
                    () =>
                      api('/maintenance', {
                        method: 'POST',
                        body: JSON.stringify({
                          target: 'fulltext',
                          action: 'clear',
                          dictionary_id: dictionary.id,
                        }),
                      }),
                    '全文索引已清除',
                  )
                }
              />
            )}
          </div>
        </div>

        {indexBusy && (
          <div className="mt-3 space-y-1.5">
            <div className="h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className="h-full bg-primary transition-[width] duration-300"
                style={{
                  width: `${
                    dictionary.index_status.total
                      ? Math.min(100, (dictionary.index_status.indexed / dictionary.index_status.total) * 100)
                      : 0
                  }%`,
                }}
              />
            </div>
            <p className="text-xs tabular-nums text-muted-foreground">
              正在构建全文索引：{dictionary.index_status.indexed.toLocaleString()} /{' '}
              {dictionary.index_status.total.toLocaleString()}
            </p>
          </div>
        )}
        {dictionary.index_status.error && <ErrorNotice error={dictionary.index_status.error} />}
      </div>

      <fieldset disabled={pending} className="min-w-0">
        <Tabs defaultValue="configuration">
          <TabsList className="mb-5">
            <TabsTrigger value="configuration">词典配置</TabsTrigger>
            <TabsTrigger value="information">元数据与说明</TabsTrigger>
          </TabsList>
          <TabsContent value="configuration" className="space-y-8">
            <Section title="基础展示与排版">
              <Field label="显示名称">
                <Input
                  aria-label="词典显示名称"
                  value={draft.name}
                  onChange={(event) => setDraft({ ...draft, name: event.target.value })}
                />
              </Field>
              <Toggle
                label="在查词中启用本词典"
                checked={draft.enabled}
                onChange={(enabled) => setDraft({ ...draft, enabled }, true)}
              />
              <Field label="词典图标">
                <div className="space-y-2">
                  <Choice
                    label="选择图标资源"
                    value={manualIcon || customIcon ? '__custom__' : draft.icon || '__auto__'}
                    items={[
                      { value: '__auto__', label: '自动识别同名或封面图标' },
                      ...imageOptions,
                      { value: '__custom__', label: '指定虚拟路径…' },
                    ]}
                    onChange={(icon) => {
                      setManualIcon(icon === '__custom__')
                      if (icon !== '__custom__') setDraft({ ...draft, icon: icon === '__auto__' ? null : icon })
                    }}
                  />
                  {(manualIcon || customIcon) && (
                    <Input
                      aria-label="词典图标资源键"
                      placeholder="/icon.png"
                      value={draft.icon ?? ''}
                      onChange={(event) => setDraft({ ...draft, icon: event.target.value || null })}
                    />
                  )}
                </div>
              </Field>
              <Field label="排版策略">
                <Choice
                  label="词典排版方式"
                  value={draft.typography_mode}
                  items={[
                    { value: 'original', label: '保留词典原始排版' },
                    { value: 'global', label: '应用全局字号与行高' },
                    { value: 'override', label: '强制覆盖正文排版与字体' },
                  ]}
                  onChange={(value) =>
                    setDraft({ ...draft, typography_mode: value as Dictionary['typography_mode'] })
                  }
                />
              </Field>
            </Section>

            <ResourceManager
              dictionary={draft}
              busy={pending || !autosave.valid}
              progress={
                pending && uploadDetail ? (
                  <OperationProgress
                    label={uploadProgress ? '正在添加资源' : '正在保存设置'}
                    detail={uploadDetail}
                    upload={uploadProgress}
                  />
                ) : undefined
              }
              onChange={(patch) => setDraft((current) => ({ ...current, ...patch }))}
              onUpload={upload}
              onBrowse={setBrowsing}
            />

            <Section
              title="词典脚本权限"
              description="默认安全模式会拦截词典内置 JS；若牛津、朗文或韦氏等交互折叠脚本需要运行，可单独为此词典开启。"
            >
              <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border px-4 py-3">
                <div className="flex items-center gap-2.5">
                  <Badge variant="secondary">
                    {dictionary.trust_scripts ? '脚本已授权运行' : '安全模式（脚本已禁用）'}
                  </Badge>
                </div>
                {dictionary.trust_scripts ? (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={pending}
                    onClick={() => void save({ trust_scripts: false })}
                  >
                    禁用脚本
                  </Button>
                ) : (
                  <Confirm
                    title={`允许“${dictionary.name}”运行脚本？`}
                    description="脚本将在无同源凭据的隔离沙盒帧中执行，仅能访问该词典自身的受限存储。备份恢复后会自动重置为禁用。"
                    label="授权运行脚本"
                    disabled={pending}
                    onConfirm={() => save({ trust_scripts: true })}
                  />
                )}
              </div>
            </Section>

            <ErrorNotice error={error} />
            <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
            {!autosave.valid && (
              <p role="alert" className="text-sm text-destructive">
                请填写词典名称并确保所有非 MDD 资源具有虚拟路径。
              </p>
            )}

            <div className="border-t pt-5">
              <Confirm
                label="移除此词典"
                title={`移除“${dictionary.name}”？`}
                description="将取消注册并删除派生索引与分组关系。原始导入文件不受影响。"
                destructive
                disabled={pending}
                onConfirm={async () => {
                  await run(async () => {
                    if (!(await autosave.flush())) throw new Error('请先处理未保存的修改。')
                    await api(base, { method: 'DELETE' })
                    back()
                  }, '词典已移除')
                }}
              />
            </div>
          </TabsContent>

          <TabsContent value="information" className="space-y-8">
            <Section title="格式与文件统计">
              <dl className="grid grid-cols-[minmax(100px,140px)_minmax(0,1fr)] gap-x-4 gap-y-3 rounded-lg border p-4 text-sm">
                {[
                  ['原始标题', metadata.title || '未提供'],
                  ['词条总数', dictionary.entries.toLocaleString()],
                  ['MDX 格式版本', metadata.format_version || '未提供'],
                  ['词典版本', metadata.dictionary_version ?? '未提供'],
                  ['字符编码', metadata.encoding || '未提供'],
                  ['加密标志', String(metadata.encrypted)],
                  ['词头块 / 记录块', `${metadata.key_blocks} / ${metadata.record_blocks}`],
                  ['创建日期', metadata.created ?? '未提供'],
                  ['托管修订号', dictionary.revision],
                ].map(([label, value]) => (
                  <div key={label} className="contents">
                    <dt className="text-muted-foreground">{label}</dt>
                    <dd className="break-all font-medium">{value}</dd>
                  </div>
                ))}
              </dl>
            </Section>
            <Section title="词典原始说明">
              <div className="rounded-lg border bg-muted/20 p-4">
                <pre className="max-w-full whitespace-pre-wrap break-words font-sans text-sm leading-7">
                  {metadata.description || '词典没有提供说明。'}
                </pre>
              </div>
            </Section>
            <Accordion type="single" collapsible className="rounded-lg border px-4">
              <AccordionItem value="attributes" className="border-0">
                <AccordionTrigger className="hover:no-underline">
                  原始头部属性（{Object.keys(metadata.attributes).length} 项）
                </AccordionTrigger>
                <AccordionContent>
                  <dl className="space-y-4 pt-2">
                    {Object.entries(metadata.attributes).map(([key, value]) => (
                      <div key={key}>
                        <dt className="break-all font-mono text-xs text-muted-foreground">{key}</dt>
                        <dd className="mt-1 whitespace-pre-wrap break-all text-sm">{value}</dd>
                      </div>
                    ))}
                  </dl>
                </AccordionContent>
              </AccordionItem>
            </Accordion>
            {dictionary.warnings.length > 0 && (
              <Section title="解析提示">
                {dictionary.warnings.map((warning, i) => (
                  <p key={i} className="break-words text-sm text-amber-700 dark:text-amber-400">
                    {warning}
                  </p>
                ))}
              </Section>
            )}
          </TabsContent>
        </Tabs>
      </fieldset>
      <ResourceBrowser
        dictionary={dictionary}
        resource={browsing}
        onClose={() => setBrowsing(null)}
        onIcon={(icon) => {
          setDraft({ ...draft, icon })
          setBrowsing(null)
        }}
      />
    </div>
  )
}

export default function DictionariesManager({
  data,
  tab,
  onTabChange,
  selected,
  onSelect,
}: {
  data: Bootstrap
  tab: string
  onTabChange: (tab: string) => void
  selected: string | null
  onSelect: (id: string | null) => void
}) {
  const { dictionaries, groups, settings } = data
  const client = useQueryClient()
  const importer = useRef<ImportHandle>(null)
  const [importActivity, setImportActivity] = useState<{
    busy: boolean
    label: string
    percent?: number
  } | null>(null)

  const dictionary =
    dictionaries.find((item) => item.id === selected) ??
    (selected === 'import' ? undefined : dictionaries[0])
  const showingImporter = selected === 'import' || dictionaries.length === 0

  async function quickToggleEnabled(target: Dictionary, enabled: boolean) {
    try {
      await api(`/dictionaries/${encodeURIComponent(target.id)}`, {
        method: 'PUT',
        body: JSON.stringify({ enabled }),
      })
      await client.invalidateQueries({ queryKey: ['bootstrap'] })
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : '状态更新失败')
    }
  }

  return (
    <Tabs value={tab === 'groups' ? 'groups' : 'manage'} onValueChange={onTabChange}>
      <TabsList className="mb-6 h-auto flex-wrap" aria-label="词典与分组">
        <TabsTrigger value="manage">
          词典库
          {dictionaries.length > 0 && (
            <span className="ml-1 text-xs text-muted-foreground">{dictionaries.length}</span>
          )}
        </TabsTrigger>
        <TabsTrigger value="groups">
          词典分组
          {groups.length > 0 && (
            <span className="ml-1 text-xs text-muted-foreground">{groups.length}</span>
          )}
        </TabsTrigger>
      </TabsList>

      <TabsContent value="manage">
        <div className="dictionary-manager">
          <aside className="dictionary-list" aria-label="词典列表">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" className="w-full justify-start">
                  <Plus className="size-4" />
                  添加词典
                  <ChevronDown className="ml-auto size-4 opacity-60" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-48">
                <DropdownMenuItem onSelect={() => importer.current?.open('files', true)}>
                  <Upload />选择词典与资源文件
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => importer.current?.open('directory', true)}>
                  <FolderSearch />扫描本地文件夹
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => importer.current?.open('server', false)}>
                  <Server />扫描服务端目录
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>

            {importActivity?.busy && !showingImporter && (
              <button
                type="button"
                onClick={() => onSelect('import')}
                className="mt-3 flex w-full items-center gap-2.5 rounded-lg border border-primary/25 bg-accent/60 px-3 py-2 text-left text-xs font-medium transition-colors hover:bg-accent"
              >
                <LoaderCircle className="size-3.5 shrink-0 animate-spin text-primary" />
                <span className="min-w-0 flex-1 truncate">{importActivity.label}</span>
                {importActivity.percent !== undefined && (
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {Math.floor(importActivity.percent)}%
                  </span>
                )}
              </button>
            )}

            {!dictionaries.length && <Empty>暂无词典</Empty>}
            {dictionaries.length > 0 && (
              <ScrollArea className="dictionary-list-scroll">
                <div className="dictionary-list-items">
                  {dictionaries.map((item) => {
                    const active = dictionary?.id === item.id && !showingImporter
                    return (
                      <div
                        key={item.id}
                        className="dictionary-list-row"
                        data-active={active}
                        data-disabled={!item.enabled}
                      >
                        <button
                          type="button"
                          className="dictionary-list-select"
                          aria-current={active ? 'true' : undefined}
                          onClick={() => onSelect(item.id)}
                        >
                          <DictionaryIcon dictionary={item} />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-sm font-medium" title={item.name}>
                              {item.name}
                            </span>
                            <span className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
                              <span className="tabular-nums">
                                {item.enabled ? `${item.entries.toLocaleString()} 词条` : '已停用'}
                              </span>
                              {item.trust_scripts && (
                                <span className="rounded bg-muted px-1 py-0.2 text-[10px] font-medium text-foreground/75">
                                  JS
                                </span>
                              )}
                              {item.index_status.state === 'ready' && (
                                <span
                                  className="size-1.5 rounded-full bg-emerald-500"
                                  title="全文索引已就绪"
                                />
                              )}
                              {item.index_status.state === 'building' && (
                                <LoaderCircle className="size-3 animate-spin text-primary" />
                              )}
                            </span>
                          </span>
                        </button>
                        <Switch
                          aria-label={`${item.enabled ? '停用' : '启用'} ${item.name}`}
                          checked={item.enabled}
                          onCheckedChange={(enabled) => void quickToggleEnabled(item, enabled)}
                        />
                      </div>
                    )
                  })}
                </div>
              </ScrollArea>
            )}
          </aside>

          <div className="dictionary-detail">
            {dictionary && !showingImporter && (
              <DictionaryDetails
                key={dictionary.id}
                dictionary={dictionary}
                back={() => onSelect(null)}
              />
            )}
            <div hidden={!showingImporter}>
              <ImportDictionaries
                ref={importer}
                existingDictionaries={dictionaries}
                onStart={() => onSelect('import')}
                onImported={(id) => {
                  onSelect(id)
                }}
                onActivityChange={setImportActivity}
              />
            </div>
          </div>
        </div>
      </TabsContent>

      <TabsContent value="groups">
        <GroupsManager groups={groups} dictionaries={dictionaries} settings={settings} />
      </TabsContent>
    </Tabs>
  )
}
