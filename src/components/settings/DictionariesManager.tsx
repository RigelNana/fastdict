import { useRef, useState } from 'react'
import { Plus, ChevronDown, Upload, FolderSearch, Server } from 'lucide-react'
import { api, apiUpload, type UploadProgress } from '@/lib/api'
import { useAutosave } from '@/lib/use-autosave'
import type { Bootstrap, Dictionary, Resource } from '@/lib/types'
import { DictionaryIcon } from '@/components/EntrySection'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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
import IndexesPanel from './IndexesPanel'
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

function DictionaryDetails({ dictionary, back, onIndexes }: { dictionary: Dictionary; back: () => void; onIndexes: () => void }) {
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
      '资源已添加',
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
  return (
    <div className="space-y-6">
      <div>
        <div className="flex min-w-0 items-start gap-3">
          <DictionaryIcon dictionary={dictionary} />
          <h3 className="min-w-0 line-clamp-2 [overflow-wrap:anywhere] text-xl font-semibold" title={dictionary.name}>{dictionary.name}</h3>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
          <span>{dictionary.entries.toLocaleString()} 个词条</span>
          <button type="button" className="rounded-md" onClick={onIndexes} aria-label={`管理 ${dictionary.name} 的全文索引`}>
            <IndexBadge status={dictionary.index_status} />
          </button>
        </div>
      </div>
      <fieldset disabled={pending} className="min-w-0">
      <Tabs defaultValue="configuration">
        <TabsList className="mb-5">
          <TabsTrigger value="configuration">配置</TabsTrigger>
          <TabsTrigger value="information">信息</TabsTrigger>
        </TabsList>
        <TabsContent value="configuration" className="space-y-8">
          <Section title="词典配置">
            <Field label="显示名称">
              <Input
                aria-label="词典显示名称"
                value={draft.name}
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
              />
            </Field>
            <Toggle
              label="启用词典"
              checked={draft.enabled}
              onChange={(enabled) => setDraft({ ...draft, enabled }, true)}
            />
            <Field label="词典图标">
              <div className="space-y-2">
                <Choice
                  label="选择图标资源"
                  value={manualIcon || customIcon ? '__custom__' : draft.icon || '__auto__'}
                  items={[
                    { value: '__auto__', label: '自动发现' },
                    ...imageOptions,
                    { value: '__custom__', label: '自定义路径' },
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
            <Field label="排版方式" >
              <Choice
                label="词典排版方式"
                value={draft.typography_mode}
                items={[
                  { value: 'original', label: '保留词典排版' },
                  { value: 'global', label: '跟随全局阅读设置' },
                  { value: 'override', label: '覆盖正文排版' },
                ]}
                onChange={(value) => setDraft({ ...draft, typography_mode: value as Dictionary['typography_mode'] })}
              />
            </Field>
          </Section>
          <ResourceManager dictionary={draft} busy={pending || !autosave.valid}
            progress={pending && uploadDetail ? <OperationProgress label={uploadProgress ? '正在添加资源' : '正在保存设置'} detail={uploadDetail} upload={uploadProgress} /> : undefined}
            onChange={(patch) => setDraft((current) => ({ ...current, ...patch }))}
            onUpload={upload} onBrowse={setBrowsing} />
          <Section
            title="脚本兼容性"

          >
            <Badge className="self-start">
              {dictionary.trust_scripts ? '脚本已启用' : '脚本已禁用'}
            </Badge>
            {dictionary.trust_scripts ? (
              <Button
                variant="outline"
                disabled={pending}
                onClick={() => void save({ trust_scripts: false })}
              >
                禁用脚本
              </Button>
            ) : (
              <Confirm
                title={`允许“${dictionary.name}”运行脚本？`}
                description="第三方词典脚本可能改变内容、执行复杂操作或消耗设备资源。此授权仅适用于这本词典，不能代替您对来源的判断。备份恢复后会重新禁用。"
                label="允许脚本"
                disabled={pending}
                onConfirm={() => save({ trust_scripts: true })}
              />
            )}
          </Section>
          <ErrorNotice error={error} />
          <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
          {!autosave.valid && <p role="alert" className="text-sm text-destructive">请填写名称和资源路径。</p>}
          <div>
            <Confirm
              label="移除词典"
              title={`移除“${dictionary.name}”？`}
              description="将取消注册并删除派生索引与分组关系。正在使用的文件版本不会被原地修改，原始上传源文件不受影响。"
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
          <Section title="格式与来源">
            <dl className="grid grid-cols-[minmax(100px,1fr)_minmax(0,2fr)] gap-x-4 gap-y-3 text-sm">
              {[
                ['原始标题', metadata.title || '未提供'],
                ['词条数', dictionary.entries.toLocaleString()],
                ['格式版本', metadata.format_version || '未提供'],
                ['词典版本', metadata.dictionary_version ?? '未提供'],
                ['编码', metadata.encoding || '未提供'],
                ['加密标志', String(metadata.encrypted)],
                ['词头块数', String(metadata.key_blocks)],
                ['记录块数', String(metadata.record_blocks)],
                ['创建日期', metadata.created ?? '未提供'],
                ['托管版本', dictionary.revision],
              ].map(([label, value]) => (
                <div key={label} className="contents">
                  <dt className="text-muted-foreground">{label}</dt>
                  <dd className="break-all">{value}</dd>
                </div>
              ))}
            </dl>
          </Section>
          <Section title="词典说明" >
            <pre className="max-w-full whitespace-pre-wrap break-words font-sans text-sm leading-7">
              {metadata.description || '词典没有提供说明。'}
            </pre>
          </Section>
          <Accordion type="single" collapsible>
            <AccordionItem value="attributes">
              <AccordionTrigger>
                原始属性（{Object.keys(metadata.attributes).length}）
              </AccordionTrigger>
              <AccordionContent>
                <dl className="space-y-4">
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
  const { dictionaries, groups } = data
  const importer = useRef<ImportHandle>(null)
  const dictionary = dictionaries.find((item) => item.id === selected) ?? (selected === 'import' ? undefined : dictionaries[0])
  return (
    <Tabs value={tab} onValueChange={onTabChange}>
      <TabsList className="mb-6 h-auto flex-wrap" aria-label="词典管理">
        <TabsTrigger value="manage">词典</TabsTrigger>
        <TabsTrigger value="groups">分组</TabsTrigger>
        <TabsTrigger value="indexes">索引与缓存</TabsTrigger>
      </TabsList>
      <TabsContent value="manage">
        <div className="dictionary-manager">
          <aside className="dictionary-list" aria-label="词典列表">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" className="w-full justify-start">
                  <Plus size={16} />添加词典<ChevronDown className="ml-auto size-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-48">
                <DropdownMenuItem onSelect={() => importer.current?.open('files')}><Upload />导入文件</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => importer.current?.open('directory')}><FolderSearch />扫描文件夹</DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => importer.current?.open('server')}><Server />扫描服务目录</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            {!dictionaries.length && <Empty>暂无词典</Empty>}
            <ScrollArea className="dictionary-list-scroll">
            <div className="dictionary-list-items">
              {dictionaries.map((item) => (
                <Button
                  key={item.id}
                  variant={dictionary?.id === item.id ? 'secondary' : 'ghost'}
                  className="dictionary-list-item"
                  aria-current={dictionary?.id === item.id ? 'true' : undefined}
                  onClick={() => onSelect(item.id)}
                >
                  <DictionaryIcon dictionary={item} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate" title={item.name}>{item.name}</span>
                    <span className="mt-1 block text-xs font-normal text-muted-foreground">
                      {item.enabled ? `${item.entries.toLocaleString()} 词条` : '已停用'}
                    </span>
                  </span>
                </Button>
              ))}
            </div>
            </ScrollArea>
          </aside>
          <div className="dictionary-detail">
            {dictionary && <DictionaryDetails key={dictionary.id} dictionary={dictionary} back={() => onSelect(null)} onIndexes={() => onTabChange('indexes')} />}
            <div hidden={!!dictionary || selected !== 'import'}>
              <ImportDictionaries ref={importer} onStart={() => onSelect('import')} onImported={(id) => { if (selected === 'import') onSelect(id) }} />
            </div>
          </div>
        </div>
      </TabsContent>
      <TabsContent value="groups">
        <GroupsManager groups={groups} dictionaries={dictionaries} />
      </TabsContent>
      <TabsContent value="indexes">
        <IndexesPanel data={data} />
      </TabsContent>
    </Tabs>
  )
}
