import { useRef, useState, type ReactNode } from 'react'
import { ArrowDown, ArrowUp, Code2, Eye, File, FileCode2, Image, MoreHorizontal, Music, Package, Plus, TextCursorInput, Trash2, Type } from 'lucide-react'
import type { Dictionary, Resource, ResourceKind } from '@/lib/types'
import { moveResourceWithinKind } from '@/lib/resource-order'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { Choice, Empty, Field, Section, bytes } from './shared'

const categories: { kind: ResourceKind; label: string; accept?: string; icon: typeof File }[] = [
  { kind: 'mdd', label: 'MDD', accept: '.mdd', icon: Package },
  { kind: 'css', label: 'CSS', accept: '.css', icon: FileCode2 },
  { kind: 'font', label: '字体', accept: '.woff,.woff2,.ttf,.otf', icon: Type },
  { kind: 'js', label: 'JS', accept: '.js', icon: Code2 },
  { kind: 'image', label: '图片', accept: '.png,.jpg,.jpeg,.gif,.webp,.avif,.bmp,.ico,.svg', icon: Image },
  { kind: 'audio', label: '音频', accept: '.mp3,.wav,.ogg,.m4a,.flac,.aac,.spx', icon: Music },
  { kind: 'file', label: '其他', icon: File },
]

const categoryMap = Object.fromEntries(categories.map((item) => [item.kind, item])) as Record<
  ResourceKind,
  (typeof categories)[number]
>

export default function ResourceManager({
  dictionary,
  busy,
  progress,
  onChange,
  onUpload,
  onBrowse,
}: {
  dictionary: Dictionary
  busy: boolean
  progress?: ReactNode
  onChange: (patch: Partial<Dictionary>) => void
  onUpload: (files: FileList | null) => Promise<void>
  onBrowse: (resource: Resource) => void
}) {
  const [filter, setFilter] = useState<'all' | ResourceKind>('all')
  const [editing, setEditing] = useState<string | null>(null)
  const upload = useRef<HTMLInputElement>(null)

  const counts = new Map<ResourceKind, number>()
  for (const resource of dictionary.resources) {
    counts.set(resource.kind, (counts.get(resource.kind) ?? 0) + 1)
  }
  const activeFilter = filter !== 'all' && !counts.get(filter) ? 'all' : filter
  const visibleResources =
    activeFilter === 'all'
      ? dictionary.resources
      : dictionary.resources.filter((resource) => resource.kind === activeFilter)

  const fonts = dictionary.resources
    .filter((resource) => resource.kind === 'font' && resource.enabled)
    .map((resource) => ({
      value: resource.key.split('/').at(-1)?.replace(/\.[^.]+$/, '') ?? resource.key,
      label: resource.path.split('/').at(-1) ?? resource.path,
    }))
    .filter((item, index, all) => all.findIndex((other) => other.value === item.value) === index)

  function patchResource(id: string, patch: Partial<Resource>) {
    onChange({
      resources: dictionary.resources.map((resource) =>
        resource.id === id ? { ...resource, ...patch } : resource,
      ),
    })
  }

  const acceptFilter = activeFilter === 'all' ? undefined : categoryMap[activeFilter]?.accept

  return (
    <Section title="挂载资源与样式">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="resource-filter-bar" role="tablist" aria-label="按资源类型筛选">
          <button
            type="button"
            role="tab"
            aria-selected={activeFilter === 'all'}
            className="resource-filter-pill"
            data-active={activeFilter === 'all'}
            onClick={() => {
              setFilter('all')
              setEditing(null)
            }}
          >
            全部
            <span className="resource-filter-count">{dictionary.resources.length}</span>
          </button>
          {categories
            .filter((item) => (counts.get(item.kind) ?? 0) > 0)
            .map((item) => (
              <button
                key={item.kind}
                type="button"
                role="tab"
                aria-selected={activeFilter === item.kind}
                className="resource-filter-pill"
                data-active={activeFilter === item.kind}
                onClick={() => {
                  setFilter(item.kind)
                  setEditing(null)
                }}
              >
                {item.label}
                <span className="resource-filter-count">{counts.get(item.kind)}</span>
              </button>
            ))}
        </div>
        <Button variant="outline" size="sm" disabled={busy} onClick={() => upload.current?.click()}>
          <Plus className="size-4" />
          添加资源
        </Button>
      </div>

      {progress}
      <input
        ref={upload}
        hidden
        type="file"
        multiple
        accept={acceptFilter}
        aria-label="上传词典资源文件"
        onChange={(event) => {
          void onUpload(event.target.files)
          event.target.value = ''
        }}
      />

      {!visibleResources.length ? (
        <div className="rounded-lg border border-dashed px-4 py-7 text-center">
          <Empty>暂未挂载伴随资源文件（支持上传 MDD、CSS、字体、JS、图标与音频）</Empty>
        </div>
      ) : (
        <div className="resource-list-card divide-y rounded-lg border">
          {visibleResources.map((resource) => {
            const meta = categoryMap[resource.kind] ?? categoryMap.file
            const Icon = meta.icon
            const kindPeers = dictionary.resources.filter((item) => item.kind === resource.kind)
            const kindIndex = kindPeers.findIndex((item) => item.id === resource.id)
            return (
              <div key={resource.id} className="px-3.5 py-3">
                <div className="flex min-w-0 items-center gap-3">
                  <Icon className="size-4 shrink-0 text-muted-foreground" />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="min-w-0 break-all text-sm font-medium">{resource.path}</span>
                      <Badge variant="secondary" className="h-5 px-1.5 text-[11px] font-medium">
                        {meta.label}
                      </Badge>
                    </div>
                    <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
                      <span className="tabular-nums">{bytes(resource.size)}</span>
                      {resource.key && resource.key !== resource.path && (
                        <span className="font-mono">→ {resource.key}</span>
                      )}
                    </div>
                  </div>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-8 text-muted-foreground hover:text-foreground"
                        aria-label={`预览 ${resource.path}`}
                        onClick={() => onBrowse(resource)}
                      >
                        <Eye className="size-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>浏览与预览</TooltipContent>
                  </Tooltip>
                  <Switch
                    aria-label={`启用资源 ${resource.path}`}
                    checked={resource.enabled}
                    onCheckedChange={(enabled) => patchResource(resource.id, { enabled })}
                  />
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon" className="size-8" aria-label={`管理 ${resource.path}`}>
                        <MoreHorizontal className="size-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent
                      align="end"
                      onCloseAutoFocus={(event) => {
                        if (editing === resource.id) event.preventDefault()
                      }}
                    >
                      <DropdownMenuItem onSelect={() => onBrowse(resource)}>
                        <Eye />浏览与预览
                      </DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => setEditing(editing === resource.id ? null : resource.id)}>
                        <TextCursorInput />编辑虚拟路径
                      </DropdownMenuItem>
                      {kindPeers.length > 1 && (
                        <>
                          <DropdownMenuItem
                            disabled={kindIndex <= 0}
                            onSelect={() =>
                              onChange({
                                resources: moveResourceWithinKind(dictionary.resources, resource.id, -1),
                              })
                            }
                          >
                            <ArrowUp />同类型上移
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            disabled={kindIndex >= kindPeers.length - 1}
                            onSelect={() =>
                              onChange({
                                resources: moveResourceWithinKind(dictionary.resources, resource.id, 1),
                              })
                            }
                          >
                            <ArrowDown />同类型下移
                          </DropdownMenuItem>
                        </>
                      )}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        variant="destructive"
                        onSelect={() =>
                          onChange({
                            resources: dictionary.resources.filter((item) => item.id !== resource.id),
                          })
                        }
                      >
                        <Trash2 />解除绑定
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                {editing === resource.id && (
                  <div className="mt-3 pl-7">
                    <Input
                      autoFocus
                      aria-label={`${resource.path} 的虚拟路径键`}
                      value={resource.key}
                      placeholder={resource.kind === 'mdd' ? '留空挂载在词典根目录' : '虚拟资源路径'}
                      onChange={(event) => patchResource(resource.id, { key: event.target.value })}
                    />
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}

      <Accordion type="multiple" className="rounded-lg border px-4">
        <AccordionItem value="font">
          <AccordionTrigger className="py-3 text-sm hover:no-underline">
            <span className="flex items-center gap-2">
              <span>专属字体覆盖</span>
              {dictionary.font_family && (
                <Badge variant="secondary" className="font-normal">
                  {dictionary.font_family}
                </Badge>
              )}
            </span>
          </AccordionTrigger>
          <AccordionContent className="space-y-4 pt-1 pb-4">
            <Field label="指定词典字体">
              <Input
                aria-label="词典内容字体覆盖"
                value={dictionary.font_family}
                placeholder="留空使用词典自带或全局字体"
                onChange={(event) => onChange({ font_family: event.target.value })}
              />
            </Field>
            {fonts.length > 0 && (
              <Field label="快速选用已挂载字体">
                <Choice
                  label="选用资源字体"
                  value={fonts.some((item) => item.value === dictionary.font_family) ? dictionary.font_family : '__manual__'}
                  items={[{ value: '__manual__', label: '使用上方手动输入' }, ...fonts]}
                  onChange={(value) => {
                    if (value !== '__manual__') {
                      onChange({ font_family: value, typography_mode: 'override' })
                    }
                  }}
                />
              </Field>
            )}
          </AccordionContent>
        </AccordionItem>
        <AccordionItem value="overrides" className="border-b-0">
          <AccordionTrigger className="py-3 text-sm hover:no-underline">
            <span className="flex items-center gap-2">
              <span>自定义补充 CSS</span>
              {dictionary.custom_css.trim() && <Badge variant="secondary">已配置</Badge>}
            </span>
          </AccordionTrigger>
          <AccordionContent className="pt-1 pb-4">
            <Textarea
              aria-label="补充 CSS"
              placeholder="在此编写仅作用于当前词典的补充 CSS 规则…"
              className="min-h-36 font-mono text-xs leading-relaxed"
              spellCheck={false}
              value={dictionary.custom_css}
              onChange={(event) => onChange({ custom_css: event.target.value })}
            />
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </Section>
  )
}
