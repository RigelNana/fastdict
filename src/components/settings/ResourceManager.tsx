import { useRef, useState } from 'react'
import { ArrowDown, ArrowUp, File, MoreHorizontal, Plus, TextCursorInput, Trash2, Eye } from 'lucide-react'
import type { Dictionary, Resource, ResourceKind } from '@/lib/types'
import { moveResourceWithinKind } from '@/lib/resource-order'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Choice, Empty, Field, Section, bytes } from './shared'

const categories: { kind: ResourceKind; label: string; accept?: string }[] = [
  { kind: 'mdd', label: 'MDD', accept: '.mdd' },
  { kind: 'css', label: 'CSS', accept: '.css' },
  { kind: 'font', label: '字体', accept: '.woff,.woff2,.ttf,.otf' },
  { kind: 'js', label: 'JS', accept: '.js' },
  { kind: 'image', label: '图片', accept: '.png,.jpg,.jpeg,.gif,.webp,.avif,.bmp,.ico,.svg' },
  { kind: 'audio', label: '音频', accept: '.mp3,.wav,.ogg,.m4a,.flac,.aac' },
  { kind: 'file', label: '其他' },
]

export default function ResourceManager({ dictionary, busy, onChange, onUpload, onBrowse }: {
  dictionary: Dictionary
  busy: boolean
  onChange: (patch: Partial<Dictionary>) => void
  onUpload: (files: FileList | null) => Promise<void>
  onBrowse: (resource: Resource) => void
}) {
  const [kind, setKind] = useState<ResourceKind>('mdd')
  const [editing, setEditing] = useState<string | null>(null)
  const upload = useRef<HTMLInputElement>(null)
  const category = categories.find((item) => item.kind === kind)!
  const fileLabel = ['mdd', 'css', 'js'].includes(kind) ? ` ${category.label} ` : category.label
  const files = dictionary.resources.filter((resource) => resource.kind === kind)
  const fonts = dictionary.resources.filter((resource) => resource.kind === 'font' && resource.enabled)
    .map((resource) => ({ value: resource.key.split('/').at(-1)?.replace(/\.[^.]+$/, '') ?? resource.key, label: resource.path.split('/').at(-1) ?? resource.path }))
    .filter((item, index, all) => all.findIndex((other) => other.value === item.value) === index)
  function patchResource(id: string, patch: Partial<Resource>) {
    onChange({ resources: dictionary.resources.map((resource) => resource.id === id ? { ...resource, ...patch } : resource) })
  }
  return (
    <Section title="资源文件">
      <Tabs value={kind} onValueChange={(value) => { setKind(value as ResourceKind); setEditing(null) }}>
        <TabsList variant="line" aria-label="资源类型" className="resource-tabs mb-5 h-auto flex-wrap justify-start gap-x-3 gap-y-2">
          {categories.map((item) => <TabsTrigger key={item.kind} value={item.kind} className="min-h-9 flex-none">{item.label}</TabsTrigger>)}
        </TabsList>
        <div className="mb-3 flex items-center justify-between gap-3">
          <span className="text-xs text-muted-foreground">{files.length} 个文件</span>
          <Button variant="ghost" size="sm" disabled={busy} onClick={() => upload.current?.click()}>
            <Plus size={16} />添加{category.label === '其他' ? '文件' : fileLabel.trimEnd()}
          </Button>
        </div>
        <input ref={upload} hidden type="file" multiple accept={category.accept} aria-label={`上传 ${category.label} 文件`}
          onChange={(event) => { void onUpload(event.target.files); event.target.value = '' }} />
        <TabsContent value={kind} className="space-y-5">
          {!files.length && <Empty>暂无{fileLabel}文件</Empty>}
          <div className="divide-y">
            {files.map((resource, index) => (
              <div key={resource.id} className="py-3">
                <div className="flex min-w-0 items-center gap-3">
                  <File className="size-4 shrink-0 text-muted-foreground" />
                  <div className="min-w-0 flex-1">
                    <p className="break-all text-sm">{resource.path}</p>
                    <span className="text-xs text-muted-foreground">{bytes(resource.size)}</span>
                  </div>
                  <Switch aria-label={`启用资源 ${resource.path}`} checked={resource.enabled}
                    onCheckedChange={(enabled) => patchResource(resource.id, { enabled })} />
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon" aria-label={`管理 ${resource.path}`}><MoreHorizontal /></Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" onCloseAutoFocus={(event) => { if (editing === resource.id) event.preventDefault() }}>
                      <DropdownMenuItem onSelect={() => onBrowse(resource)}><Eye />浏览与预览</DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => setEditing(editing === resource.id ? null : resource.id)}><TextCursorInput />编辑路径</DropdownMenuItem>
                      <DropdownMenuItem disabled={index === 0} onSelect={() => onChange({ resources: moveResourceWithinKind(dictionary.resources, resource.id, -1) })}><ArrowUp />上移</DropdownMenuItem>
                      <DropdownMenuItem disabled={index === files.length - 1} onSelect={() => onChange({ resources: moveResourceWithinKind(dictionary.resources, resource.id, 1) })}><ArrowDown />下移</DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem variant="destructive" onSelect={() => onChange({ resources: dictionary.resources.filter((item) => item.id !== resource.id) })}><Trash2 />解除绑定</DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                {editing === resource.id && <div className="mt-3">
                  <Input autoFocus aria-label={`${resource.path} 的虚拟路径键`} value={resource.key}
                    placeholder={kind === 'mdd' ? '根目录' : '文件路径'}
                    onChange={(event) => patchResource(resource.id, { key: event.target.value })} />
                </div>}
              </div>
            ))}
          </div>
          {kind === 'css' && <Accordion type="single" collapsible>
            <AccordionItem value="overrides" className="border-0">
              <AccordionTrigger className="py-2">补充样式</AccordionTrigger>
              <AccordionContent className="pt-2">
                <Textarea aria-label="补充 CSS" className="min-h-40 font-mono text-sm" spellCheck={false}
                  value={dictionary.custom_css} onChange={(event) => onChange({ custom_css: event.target.value })} />
              </AccordionContent>
            </AccordionItem>
          </Accordion>}
          {kind === 'font' && <div className="space-y-4">
            <Field label="正文字体">
              <Input aria-label="词典内容字体覆盖" value={dictionary.font_family} placeholder="使用词典字体"
                onChange={(event) => onChange({ font_family: event.target.value })} />
            </Field>
            {fonts.length > 0 && <Field label="选用字体文件">
              <Choice label="选用资源字体" value={fonts.some((item) => item.value === dictionary.font_family) ? dictionary.font_family : '__manual__'}
                items={[{ value: '__manual__', label: '使用上方字体设置' }, ...fonts]}
                onChange={(value) => { if (value !== '__manual__') onChange({ font_family: value, typography_mode: 'override' }) }} />
            </Field>}
          </div>}
        </TabsContent>
      </Tabs>
    </Section>
  )
}
