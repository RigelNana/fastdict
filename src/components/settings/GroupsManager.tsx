import { useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { DndContext, KeyboardSensor, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core'
import { SortableContext, arrayMove, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { ArrowDown, ArrowUp, Check, FolderOpen, GripVertical, Plus, Star, X } from 'lucide-react'
import { toast } from 'sonner'
import { api } from '@/lib/api'
import { useAutosave } from '@/lib/use-autosave'
import type { Dictionary, Group, Settings } from '@/lib/types'
import { DictionaryIcon } from '@/components/EntrySection'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { Choice, Confirm, Empty, ErrorNotice, Field, SaveStatus, Section, useAction } from './shared'

function OrderedDictionary({
  dictionary,
  index,
  count,
  autoSorted,
  move,
  remove,
}: {
  dictionary: Dictionary
  index: number
  count: number
  autoSorted: boolean
  move: (from: number, to: number) => void
  remove: () => void
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: dictionary.id,
    disabled: autoSorted,
  })
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className="group-member"
      data-dragging={isDragging}
    >
      <Button
        variant="ghost"
        size="icon"
        {...attributes}
        {...listeners}
        disabled={autoSorted}
        aria-label={`拖动调整 ${dictionary.name} 的顺序`}
        className="size-8 shrink-0 touch-none text-muted-foreground"
      >
        <GripVertical className="size-4" />
      </Button>
      <span className="w-5 shrink-0 text-center text-xs font-medium tabular-nums text-muted-foreground">
        {index + 1}
      </span>
      <DictionaryIcon dictionary={dictionary} />
      <div className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium" title={dictionary.name}>
          {dictionary.name}
        </span>
        <span className="text-xs text-muted-foreground">
          {dictionary.enabled ? `${dictionary.entries.toLocaleString()} 词条` : '已停用'}
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-0.5">
        {!autoSorted && count > 1 && (
          <>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-8 text-muted-foreground hover:text-foreground"
                  disabled={index === 0}
                  aria-label={`上移 ${dictionary.name}`}
                  onClick={() => move(index, index - 1)}
                >
                  <ArrowUp className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>上移</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-8 text-muted-foreground hover:text-foreground"
                  disabled={index === count - 1}
                  aria-label={`下移 ${dictionary.name}`}
                  onClick={() => move(index, index + 1)}
                >
                  <ArrowDown className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>下移</TooltipContent>
            </Tooltip>
          </>
        )}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-8 text-muted-foreground hover:text-destructive"
              aria-label={`将 ${dictionary.name} 移出分组`}
              onClick={remove}
            >
              <X className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>移出分组</TooltipContent>
        </Tooltip>
      </div>
    </li>
  )
}

function GroupEditor({
  group,
  dictionaries,
  settings,
}: {
  group: Group
  dictionaries: Dictionary[]
  settings: Settings
}) {
  const client = useQueryClient()
  const autosave = useAutosave({
    value: group,
    resource: `/groups/${group.id}`,
    valid: (value) => !!value.name.trim(),
    save: (value) =>
      api(`/groups/${encodeURIComponent(group.id)}`, {
        method: 'PUT',
        body: JSON.stringify({
          name: value.name.trim(),
          dictionary_ids: value.dictionary_ids,
          sort: value.sort,
        }),
      }),
  })
  const { draft, change: setDraft } = autosave
  const { pending, error, run } = useAction()
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )

  const isDefault = settings.application.default_group === group.id
  const autoSorted = draft.sort.length > 0

  function move(from: number, to: number) {
    if (from < 0 || to < 0 || to >= draft.dictionary_ids.length) return
    setDraft((current) => ({ ...current, dictionary_ids: arrayMove(current.dictionary_ids, from, to) }), true)
  }

  function dragEnd(event: DragEndEvent) {
    if (event.over && event.active.id !== event.over.id) {
      move(
        draft.dictionary_ids.indexOf(String(event.active.id)),
        draft.dictionary_ids.indexOf(String(event.over.id)),
      )
    }
  }

  async function toggleDefaultGroup() {
    try {
      await api<Settings>('/settings', {
        method: 'PUT',
        body: JSON.stringify({
          ...settings,
          application: {
            ...settings.application,
            default_group: isDefault ? 'all' : group.id,
          },
        }),
      })
      await client.invalidateQueries({ queryKey: ['bootstrap'] })
      toast.success(isDefault ? '已恢复默认查询全部词典' : `已将“${draft.name || group.name}”设为默认分组`)
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : '设置默认分组失败')
    }
  }

  const members = draft.dictionary_ids.flatMap(
    (id) => dictionaries.find((dictionary) => dictionary.id === id) ?? [],
  )
  const available = dictionaries.filter(
    (dictionary) => !draft.dictionary_ids.includes(dictionary.id),
  )

  return (
    <div className="space-y-6">
      <div className="rounded-xl border bg-muted/25 p-4 sm:p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="flex min-w-0 flex-1 items-start gap-3.5">
            <div className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg border bg-background">
              <FolderOpen className="size-4 text-muted-foreground" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <h3
                  className="min-w-0 line-clamp-2 [overflow-wrap:anywhere] text-lg font-semibold leading-snug"
                  title={draft.name}
                >
                  {draft.name || group.name}
                </h3>
                {isDefault && <Badge variant="secondary">默认查询分组</Badge>}
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                已包含 {members.length} / {dictionaries.length} 部词典
                {autoSorted ? ` · 已启用 ${draft.sort.length} 条自动排序规则` : ' · 按手动顺序展示'}
              </p>
            </div>
          </div>
          <Button variant={isDefault ? 'secondary' : 'outline'} size="sm" onClick={() => void toggleDefaultGroup()}>
            {isDefault ? <Check className="size-3.5" /> : <Star className="size-3.5" />}
            {isDefault ? '取消默认' : '设为默认分组'}
          </Button>
        </div>
      </div>

      <Section title="分组名称">
        <Field label="显示名称">
          <Input
            aria-label="分组名称"
            value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.target.value })}
          />
        </Field>
      </Section>

      <section className="settings-section">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h3 className="settings-section-title">本组词典与展示顺序（{members.length}）</h3>
            <p className="mt-0.5 text-xs text-muted-foreground">
              {autoSorted
                ? '当前由下方自动排序规则决定查词先后顺序。'
                : '上下拖动词典或点击箭头即可调整查词结果的上下排列顺序。'}
            </p>
          </div>
          {members.length > 0 && (
            <Button
              variant="ghost"
              size="sm"
              className="text-xs text-muted-foreground"
              onClick={() => setDraft((current) => ({ ...current, dictionary_ids: [] }), true)}
            >
              清空本组
            </Button>
          )}
        </div>

        {members.length > 0 ? (
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={dragEnd}>
            <SortableContext items={members.map((d) => d.id)} strategy={verticalListSortingStrategy}>
              <ul className="divide-y overflow-hidden rounded-xl border">
                {members.map((dictionary, index) => (
                  <OrderedDictionary
                    key={dictionary.id}
                    dictionary={dictionary}
                    index={index}
                    count={members.length}
                    autoSorted={autoSorted}
                    move={move}
                    remove={() =>
                      setDraft(
                        (current) => ({
                          ...current,
                          dictionary_ids: current.dictionary_ids.filter((id) => id !== dictionary.id),
                        }),
                        true,
                      )
                    }
                  />
                ))}
              </ul>
            </SortableContext>
          </DndContext>
        ) : (
          <div className="rounded-xl border border-dashed px-4 py-6 text-center">
            <Empty>本分组还没有词典，请从下方点击“加入”添加词典</Empty>
          </div>
        )}
      </section>

      {available.length > 0 && (
        <section className="settings-section">
          <div className="flex items-center justify-between gap-3">
            <h3 className="settings-section-title">可加入本组的词典（{available.length}）</h3>
            {available.length > 1 && (
              <Button
                variant="outline"
                size="sm"
                onClick={() =>
                  setDraft(
                    (current) => ({
                      ...current,
                      dictionary_ids: [
                        ...current.dictionary_ids,
                        ...available.map((item) => item.id),
                      ],
                    }),
                    true,
                  )
                }
              >
                <Plus className="size-3.5" />
                全部加入
              </Button>
            )}
          </div>
          <ul className="divide-y overflow-hidden rounded-xl border">
            {available.map((dictionary) => (
              <li
                key={dictionary.id}
                className="flex min-w-0 items-center gap-3 px-3.5 py-2.5 hover:bg-muted/40"
              >
                <DictionaryIcon dictionary={dictionary} />
                <div className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium" title={dictionary.name}>
                    {dictionary.name}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {dictionary.enabled ? `${dictionary.entries.toLocaleString()} 词条` : '已停用'}
                  </span>
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  className="h-8 px-3 text-xs"
                  onClick={() =>
                    setDraft(
                      (current) => ({
                        ...current,
                        dictionary_ids: [...current.dictionary_ids, dictionary.id],
                      }),
                      true,
                    )
                  }
                >
                  <Plus className="size-3.5" />
                  加入
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <Accordion type="single" collapsible className="rounded-xl border px-4">
        <AccordionItem value="sort" className="border-0">
          <AccordionTrigger className="py-3.5 hover:no-underline">
            <span className="flex min-w-0 flex-1 items-center justify-between gap-3 pr-2">
              <span className="text-sm font-semibold">自动排序规则（可选）</span>
              <span className="text-xs font-normal text-muted-foreground">
                {draft.sort.length ? `已设 ${draft.sort.length} 条规则` : '默认使用上方手动顺序'}
              </span>
            </span>
          </AccordionTrigger>
          <AccordionContent className="space-y-3 pb-4">
            {draft.sort.map((rule, index) => (
              <div key={index} className="group-sort-rule">
                <Choice
                  label={`第 ${index + 1} 条排序字段`}
                  value={rule.field}
                  items={[
                    { value: 'name', label: '按词典名称' },
                    { value: 'entries', label: '按词条数量' },
                    { value: 'created', label: '按导入时间' },
                  ]}
                  onChange={(field) =>
                    setDraft(
                      (current) => ({
                        ...current,
                        sort: current.sort.map((item, i) =>
                          i === index ? { ...item, field: field as typeof rule.field } : item,
                        ),
                      }),
                      true,
                    )
                  }
                />
                <Choice
                  label={`第 ${index + 1} 条排序方向`}
                  value={rule.direction}
                  items={[
                    { value: 'asc', label: '升序' },
                    { value: 'desc', label: '降序' },
                  ]}
                  onChange={(direction) =>
                    setDraft(
                      (current) => ({
                        ...current,
                        sort: current.sort.map((item, i) =>
                          i === index ? { ...item, direction: direction as typeof rule.direction } : item,
                        ),
                      }),
                      true,
                    )
                  }
                />
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`移除第 ${index + 1} 条排序规则`}
                  onClick={() =>
                    setDraft({ ...draft, sort: draft.sort.filter((_, i) => i !== index) }, true)
                  }
                >
                  <X className="size-4" />
                </Button>
              </div>
            ))}
            <Button
              variant="outline"
              size="sm"
              disabled={draft.sort.length >= 3}
              onClick={() => {
                const field = (['name', 'entries', 'created'] as const).find(
                  (candidateField) => !draft.sort.some((rule) => rule.field === candidateField),
                )
                if (field) {
                  setDraft({ ...draft, sort: [...draft.sort, { field, direction: 'asc' }] }, true)
                }
              }}
            >
              <Plus className="size-4" />
              添加自动排序规则
            </Button>
          </AccordionContent>
        </AccordionItem>
      </Accordion>

      <ErrorNotice error={error} />
      <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
      {!autosave.valid && <p role="alert" className="text-sm text-destructive">请填写分组名称。</p>}

      <div className="border-t pt-5">
        <Confirm
          label="删除此分组"
          title={`删除分组“${group.name}”？`}
          description="仅移除分组配置，分组内的词典本身不会受到任何影响。"
          destructive
          disabled={pending}
          onConfirm={() =>
            run(async () => {
              if (!(await autosave.flush())) throw new Error('请先填写分组名称。')
              return api(`/groups/${encodeURIComponent(group.id)}`, { method: 'DELETE' })
            }, '分组已删除')
          }
        />
      </div>
    </div>
  )
}

export default function GroupsManager({
  groups,
  dictionaries,
  settings,
}: {
  groups: Group[]
  dictionaries: Dictionary[]
  settings: Settings
}) {
  const [parameters, setParameters] = useSearchParams()
  const selected = groups.find((group) => group.id === parameters.get('group')) ?? groups[0]
  const select = (id: string) =>
    setParameters(
      (current) => {
        const next = new URLSearchParams(current)
        next.set('group', id)
        return next
      },
      { replace: true },
    )
  const [name, setName] = useState('')
  const [creating, setCreating] = useState(false)
  const { pending, error, run } = useAction()

  const createDialog = (
    <Dialog
      open={creating}
      onOpenChange={(open) => {
        if (!pending) {
          setCreating(open)
          if (!open) setName('')
        }
      }}
    >
      <DialogTrigger asChild>
        <Button variant="outline" className="w-full justify-start">
          <Plus className="size-4" />
          新建分组
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>新建词典分组</DialogTitle>
          <DialogDescription>例如“英英精读”、“双解速查”或“专业词汇”，创建后可选择包含的词典与排序。</DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={async (event) => {
            event.preventDefault()
            if (pending || !name.trim()) return
            const result = await run(
              () =>
                api<Group>('/groups', {
                  method: 'POST',
                  body: JSON.stringify({
                    name: name.trim(),
                    dictionary_ids: [],
                    sort: [],
                  }),
                }),
              '分组已创建',
            )
            if (result) {
              select(result.id)
              setName('')
              setCreating(false)
            }
          }}
        >
          <Input
            autoFocus
            aria-label="新分组名称"
            placeholder="输入分组名称…"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <ErrorNotice error={error} />
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              disabled={pending}
              onClick={() => {
                setCreating(false)
                setName('')
              }}
            >
              取消
            </Button>
            <Button type="submit" disabled={pending || !name.trim()}>
              {pending ? '创建中…' : '创建分组'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )

  return (
    <div className="dictionary-manager">
      <aside className="dictionary-list" aria-label="分组列表">
        {createDialog}
        {groups.length > 0 && (
          <ScrollArea className="dictionary-list-scroll">
            <div className="dictionary-list-items">
              {groups.map((group) => {
                const isDefault = settings.application.default_group === group.id
                return (
                  <Button
                    key={group.id}
                    variant={selected?.id === group.id ? 'secondary' : 'ghost'}
                    className="dictionary-list-item"
                    aria-current={selected?.id === group.id ? 'true' : undefined}
                    onClick={() => select(group.id)}
                  >
                    <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1.5">
                        <span className="truncate font-medium" title={group.name}>
                          {group.name}
                        </span>
                        {isDefault && (
                          <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium text-foreground/80">
                            默认
                          </span>
                        )}
                      </span>
                      <span className="mt-0.5 block text-xs font-normal text-muted-foreground">
                        {group.dictionary_ids.length} 部词典
                      </span>
                    </span>
                  </Button>
                )
              })}
            </div>
          </ScrollArea>
        )}
        {!groups.length && <Empty>暂无自定义分组</Empty>}
      </aside>

      <div className="dictionary-detail">
        {selected ? (
          <GroupEditor key={selected.id} group={selected} dictionaries={dictionaries} settings={settings} />
        ) : (
          <div className="rounded-xl border border-dashed p-8 text-center">
            <FolderOpen className="mx-auto size-8 text-muted-foreground" />
            <h3 className="mt-3 text-base font-semibold">按场景组织你的词典分组</h3>
            <p className="mx-auto mt-1.5 max-w-md text-sm text-muted-foreground">
              创建分组后，可在查词框右侧一键切换当前查询范围（如“主力双解”、“英英原版”），每个分组都拥有独立的启用列表和先后展示顺序。
            </p>
            <Button className="mt-5" onClick={() => setCreating(true)}>
              <Plus className="size-4" />
              创建第一个分组
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}
