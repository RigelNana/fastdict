import { useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { DndContext, KeyboardSensor, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core'
import { SortableContext, arrayMove, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { ArrowDown, ArrowUp, FolderOpen, GripVertical, MoreHorizontal, Plus, X } from 'lucide-react'
import { api } from '@/lib/api'
import { useAutosave } from '@/lib/use-autosave'
import type { Dictionary, Group } from '@/lib/types'
import { DictionaryIcon } from '@/components/EntrySection'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Choice, Confirm, Empty, ErrorNotice, Field, SaveStatus, Section, useAction } from './shared'

function OrderedDictionary({ dictionary, index, count, move, remove }: {
  dictionary: Dictionary; index: number; count: number
  move: (from: number, to: number) => void; remove: () => void
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: dictionary.id })
  return (
    <li ref={setNodeRef} style={{ transform: CSS.Transform.toString(transform), transition }}
      className="group-member" data-dragging={isDragging}>
      <Button variant="ghost" size="icon" {...attributes} {...listeners}
        aria-label={`拖动调整 ${dictionary.name} 的顺序`} className="shrink-0 touch-none text-muted-foreground">
        <GripVertical className="size-4" />
      </Button>
      <DictionaryIcon dictionary={dictionary} />
      <span className="min-w-0 flex-1 truncate text-sm" title={dictionary.name}>{dictionary.name}</span>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" aria-label={`管理分组成员 ${dictionary.name}`}><MoreHorizontal className="size-4" /></Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem disabled={index === 0} onSelect={() => move(index, index - 1)}><ArrowUp />上移</DropdownMenuItem>
          <DropdownMenuItem disabled={index === count - 1} onSelect={() => move(index, index + 1)}><ArrowDown />下移</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={remove}><X />移出分组</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  )
}

function GroupEditor({ group, dictionaries }: { group: Group; dictionaries: Dictionary[] }) {
  const autosave = useAutosave({
    value: group, resource: `/groups/${group.id}`, valid: value => !!value.name.trim(),
    save: value => api(`/groups/${encodeURIComponent(group.id)}`, {
      method: 'PUT', body: JSON.stringify({ name: value.name.trim(), dictionary_ids: value.dictionary_ids, sort: value.sort }),
    }),
  })
  const { draft, change: setDraft } = autosave
  const { pending, error, run } = useAction()
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }))
  function move(from: number, to: number) {
    if (from < 0 || to < 0 || to >= draft.dictionary_ids.length) return
    setDraft(current => ({ ...current, dictionary_ids: arrayMove(current.dictionary_ids, from, to) }), true)
  }
  function dragEnd(event: DragEndEvent) {
    if (event.over && event.active.id !== event.over.id)
      move(draft.dictionary_ids.indexOf(String(event.active.id)), draft.dictionary_ids.indexOf(String(event.over.id)))
  }
  const members = draft.dictionary_ids.flatMap(id => dictionaries.find(dictionary => dictionary.id === id) ?? [])
  const available = dictionaries.filter(dictionary => !draft.dictionary_ids.includes(dictionary.id))
  return (
    <div className="space-y-6">
      <div>
        <div className="flex min-w-0 items-start gap-3">
          <FolderOpen className="dictionary-icon text-muted-foreground" />
          <h3 className="min-w-0 line-clamp-2 [overflow-wrap:anywhere] text-xl font-semibold" title={draft.name}>{draft.name || group.name}</h3>
        </div>
        <p className="mt-2 text-sm text-muted-foreground">{members.length} 部词典</p>
      </div>
      <Section title="分组配置">
        <Field label="显示名称"><Input aria-label="分组名称" value={draft.name} onChange={event => setDraft({ ...draft, name: event.target.value })} /></Field>
      </Section>
      <section className="settings-section">
        <div className="flex items-center justify-between gap-3">
          <h3 className="settings-section-title">词典</h3>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label="添加分组成员" title="添加词典" disabled={!available.length}><Plus className="size-4" /></Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="group-member-menu">
              {available.map(dictionary => <DropdownMenuItem key={dictionary.id}
                onSelect={event => { event.preventDefault(); setDraft(current => ({ ...current, dictionary_ids: [...current.dictionary_ids, dictionary.id] }), true) }}>
                <DictionaryIcon dictionary={dictionary} />
                <span className="min-w-0 flex-1 truncate" title={dictionary.name}>{dictionary.name}</span>
              </DropdownMenuItem>)}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={dragEnd}>
          <SortableContext items={members.map(dictionary => dictionary.id)} strategy={verticalListSortingStrategy}>
            <ul className="space-y-1">{members.map((dictionary, index) => <OrderedDictionary key={dictionary.id}
              dictionary={dictionary} index={index} count={members.length} move={move}
              remove={() => setDraft(current => ({ ...current, dictionary_ids: current.dictionary_ids.filter(id => id !== dictionary.id) }), true)} />)}</ul>
          </SortableContext>
        </DndContext>
        {!members.length && <Empty>暂无词典</Empty>}
      </section>
      <Accordion type="single" collapsible>
        <AccordionItem value="sort" className="border-0">
          <AccordionTrigger className="pt-0 hover:no-underline">
            <span className="flex min-w-0 flex-1 items-center justify-between gap-3">
              <span className="settings-section-title">排序规则</span>
              <span className="text-xs font-normal text-muted-foreground">{draft.sort.length ? `${draft.sort.length} 条规则` : '手动排序'}</span>
            </span>
          </AccordionTrigger>
          <AccordionContent className="space-y-3">
            {draft.sort.map((rule, index) => <div key={index} className="group-sort-rule">
              <Choice label={`第 ${index + 1} 条排序字段`} value={rule.field}
                items={[{ value: 'name', label: '名称' }, { value: 'entries', label: '词条数' }, { value: 'created', label: '创建日期' }]}
                onChange={field => setDraft(current => ({ ...current, sort: current.sort.map((item, i) => i === index ? { ...item, field: field as typeof rule.field } : item) }), true)} />
              <Choice label={`第 ${index + 1} 条排序方向`} value={rule.direction}
                items={[{ value: 'asc', label: '升序' }, { value: 'desc', label: '降序' }]}
                onChange={direction => setDraft(current => ({ ...current, sort: current.sort.map((item, i) => i === index ? { ...item, direction: direction as typeof rule.direction } : item) }), true)} />
              <DropdownMenu>
                <DropdownMenuTrigger asChild><Button variant="ghost" size="icon" aria-label={`第 ${index + 1} 条规则操作`}><MoreHorizontal className="size-4" /></Button></DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem disabled={index === 0} onSelect={() => setDraft({ ...draft, sort: arrayMove(draft.sort, index, index - 1) }, true)}><ArrowUp />上移</DropdownMenuItem>
                  <DropdownMenuItem disabled={index === draft.sort.length - 1} onSelect={() => setDraft({ ...draft, sort: arrayMove(draft.sort, index, index + 1) }, true)}><ArrowDown />下移</DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onSelect={() => setDraft({ ...draft, sort: draft.sort.filter((_, i) => i !== index) }, true)}><X />移除规则</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>)}
            <Button variant="outline" disabled={draft.sort.length >= 3} onClick={() => {
              const field = (['name', 'entries', 'created'] as const).find(field => !draft.sort.some(rule => rule.field === field))
              if (field) setDraft({ ...draft, sort: [...draft.sort, { field, direction: 'asc' }] }, true)
            }}><Plus className="size-4" />添加规则</Button>
          </AccordionContent>
        </AccordionItem>
      </Accordion>
      <ErrorNotice error={error} />
      <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
      {!autosave.valid && <p role="alert" className="text-sm text-destructive">请填写分组名称。</p>}
      <div className="border-t pt-5">
        <Confirm label="删除分组" title={`删除“${group.name}”？`} description="词典文件会保留。" destructive disabled={pending}
          onConfirm={() => run(async () => {
            if (!(await autosave.flush())) throw new Error('请先填写分组名称。')
            return api(`/groups/${encodeURIComponent(group.id)}`, { method: 'DELETE' })
          }, '分组已删除')} />
      </div>
    </div>
  )
}

export default function GroupsManager({ groups, dictionaries }: { groups: Group[]; dictionaries: Dictionary[] }) {
  const [parameters, setParameters] = useSearchParams()
  const selected = groups.find(group => group.id === parameters.get('group')) ?? groups[0]
  const select = (id: string) => setParameters(current => { const next = new URLSearchParams(current); next.set('group', id); return next }, { replace: true })
  const [name, setName] = useState('')
  const [creating, setCreating] = useState(false)
  const { pending, error, run } = useAction()
  return (
    <div className="dictionary-manager">
      <aside className="dictionary-list" aria-label="分组列表">
        <div className="flex min-h-9 justify-end">
          <Dialog open={creating} onOpenChange={open => { if (!pending) { setCreating(open); if (!open) setName('') } }}>
            <DialogTrigger asChild><Button variant="ghost" size="icon" aria-label="创建分组" title="创建分组"><Plus className="size-4" /></Button></DialogTrigger>
            <DialogContent className="sm:max-w-sm">
              <DialogHeader><DialogTitle>创建分组</DialogTitle><DialogDescription className="sr-only">输入新分组的名称</DialogDescription></DialogHeader>
              <form className="space-y-4" onSubmit={async event => {
                event.preventDefault()
                if (pending || !name.trim()) return
                const result = await run(() => api<Group>('/groups', { method: 'POST', body: JSON.stringify({ name: name.trim(), dictionary_ids: [], sort: [] }) }), '分组已创建')
                if (result) { select(result.id); setName(''); setCreating(false) }
              }}>
                <Input autoFocus aria-label="新分组名称" placeholder="分组名称" value={name} onChange={event => setName(event.target.value)} />
                <ErrorNotice error={error} />
                <DialogFooter>
                  <Button type="button" variant="ghost" disabled={pending} onClick={() => { setCreating(false); setName('') }}>取消</Button>
                  <Button type="submit" disabled={pending || !name.trim()}>{pending ? '创建中…' : '创建'}</Button>
                </DialogFooter>
              </form>
            </DialogContent>
          </Dialog>
        </div>
        <ScrollArea className="dictionary-list-scroll"><div className="dictionary-list-items">
          {groups.map(group => <Button key={group.id} variant={selected?.id === group.id ? 'secondary' : 'ghost'}
            className="dictionary-list-item" aria-current={selected?.id === group.id ? 'true' : undefined} onClick={() => select(group.id)}>
            <FolderOpen className="size-5 shrink-0 text-muted-foreground" />
            <span className="min-w-0 flex-1">
              <span className="block truncate" title={group.name}>{group.name}</span>
              <span className="mt-1 block text-xs font-normal text-muted-foreground">{group.dictionary_ids.length} 部词典</span>
            </span>
          </Button>)}
        </div></ScrollArea>
        {!groups.length && <Empty>暂无分组</Empty>}
      </aside>
      <div className="dictionary-detail">{selected && <GroupEditor key={selected.id} group={selected} dictionaries={dictionaries} />}</div>
    </div>
  )
}
