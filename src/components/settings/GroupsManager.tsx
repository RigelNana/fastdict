import { useState } from 'react'
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { ArrowDown, ArrowUp, GripVertical, Plus, X } from 'lucide-react'
import { api } from '@/lib/api'
import { useAutosave } from '@/lib/use-autosave'
import type { Dictionary, Group } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Choice, Confirm, Empty, ErrorNotice, SaveStatus, useAction } from './shared'

function OrderedDictionary({
  dictionary,
  index,
  count,
  move,
  remove,
}: {
  dictionary: Dictionary
  index: number
  count: number
  move: (from: number, to: number) => void
  remove: () => void
}) {
  const { attributes, listeners, setNodeRef, transform, transition } = useSortable({
    id: dictionary.id,
  })
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className="flex min-w-0 items-center gap-2 border-b bg-background py-2"
    >
      <Button
        variant="ghost"
        size="icon"
        {...attributes}
        {...listeners}
        aria-label={`拖动调整 ${dictionary.name} 的顺序`}
        className="shrink-0 touch-none"
      >
        <GripVertical size={16} />
      </Button>
      <span className="min-w-0 flex-1 break-words text-sm">{dictionary.name}</span>
      <Button
        variant="ghost"
        size="icon"
        aria-label={`上移 ${dictionary.name}`}
        disabled={index === 0}
        onClick={() => move(index, index - 1)}
      >
        <ArrowUp size={16} />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label={`下移 ${dictionary.name}`}
        disabled={index === count - 1}
        onClick={() => move(index, index + 1)}
      >
        <ArrowDown size={16} />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label={`从组中移除 ${dictionary.name}`}
        onClick={remove}
      >
        <X size={16} />
      </Button>
    </li>
  )
}
function GroupEditor({ group, dictionaries }: { group: Group; dictionaries: Dictionary[] }) {
  const autosave = useAutosave({
    value: group,
    resource: `/groups/${group.id}`,
    valid: (value) => !!value.name.trim(),
    save: (value) => api(`/groups/${encodeURIComponent(group.id)}`, {
      method: 'PUT', body: JSON.stringify({ name: value.name.trim(), dictionary_ids: value.dictionary_ids, sort: value.sort }),
    }),
  })
  const { draft, change: setDraft } = autosave
  const { pending, error, run } = useAction()
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )
  function move(from: number, to: number) {
    setDraft((current) => ({
      ...current,
      dictionary_ids: arrayMove(current.dictionary_ids, from, to),
    }))
  }
  function dragEnd(event: DragEndEvent) {
    if (event.over && event.active.id !== event.over.id)
      move(
        draft.dictionary_ids.indexOf(String(event.active.id)),
        draft.dictionary_ids.indexOf(String(event.over.id)),
      )
  }
  const members = draft.dictionary_ids
    .map((id) => dictionaries.find((dictionary) => dictionary.id === id))
    .filter((dictionary): dictionary is Dictionary => !!dictionary)
  return (
    <div className="space-y-5 border-b py-6">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          className="flex-1"
          aria-label="分组名称"
          value={draft.name}
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
        />
        <Confirm
          label="删除分组"
          title={`删除“${group.name}”？`}
          description="只删除分组，不会删除词典或其他分组的成员关系。"
          destructive
          disabled={pending}
          onConfirm={() =>
            run(
              async () => {
                if (!(await autosave.flush())) throw new Error('请先填写分组名称。')
                return api(`/groups/${encodeURIComponent(group.id)}`, { method: 'DELETE' })
              },
              '分组已删除',
            )
          }
        />
      </div>

      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={dragEnd}>
        <SortableContext
          items={members.map((dictionary) => dictionary.id)}
          strategy={verticalListSortingStrategy}
        >
          <ul>
            {members.map((dictionary, index) => (
              <OrderedDictionary
                key={dictionary.id}
                dictionary={dictionary}
                index={index}
                count={members.length}
                move={move}
                remove={() =>
                  setDraft({
                    ...draft,
                    dictionary_ids: draft.dictionary_ids.filter((id) => id !== dictionary.id),
                  })
                }
              />
            ))}
          </ul>
        </SortableContext>
      </DndContext>
      {!members.length && <p className="text-sm text-muted-foreground">尚未加入词典。</p>}
      <div className="flex flex-wrap gap-x-6 gap-y-3">
        {dictionaries
          .filter((dictionary) => !draft.dictionary_ids.includes(dictionary.id))
          .map((dictionary) => (
            <label key={dictionary.id} className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={false}
                onCheckedChange={() =>
                  setDraft({ ...draft, dictionary_ids: [...draft.dictionary_ids, dictionary.id] })
                }
              />
              {dictionary.name}
            </label>
          ))}
      </div>
      <div className="space-y-3">
        <h3 className="settings-section-title">排序规则</h3>
        <p className="text-sm text-muted-foreground">
          从上到下优先；相同时按手动顺序。
        </p>
        {draft.sort.map((rule, index) => (
          <div key={index} className="flex flex-wrap items-center gap-2">
            <div className="min-w-32 flex-1">
              <Choice
                label={`第 ${index + 1} 条排序字段`}
                value={rule.field}
                items={[
                  { value: 'name', label: '名称' },
                  { value: 'entries', label: '词条数' },
                  { value: 'created', label: '创建日期' },
                ]}
                onChange={(field) =>
                  setDraft({
                    ...draft,
                    sort: draft.sort.map((item, i) =>
                      i === index ? { ...item, field: field as typeof rule.field } : item,
                    ),
                  })
                }
              />
            </div>
            <div className="w-28">
              <Choice
                label={`第 ${index + 1} 条排序方向`}
                value={rule.direction}
                items={[
                  { value: 'asc', label: '升序' },
                  { value: 'desc', label: '降序' },
                ]}
                onChange={(direction) =>
                  setDraft({
                    ...draft,
                    sort: draft.sort.map((item, i) =>
                      i === index
                        ? { ...item, direction: direction as typeof rule.direction }
                        : item,
                    ),
                  })
                }
              />
            </div>
            <Button
              variant="ghost"
              size="icon"
              aria-label="提高规则优先级"
              disabled={index === 0}
              onClick={() => setDraft({ ...draft, sort: arrayMove(draft.sort, index, index - 1) })}
            >
              <ArrowUp size={16} />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              aria-label="降低规则优先级"
              disabled={index === draft.sort.length - 1}
              onClick={() => setDraft({ ...draft, sort: arrayMove(draft.sort, index, index + 1) })}
            >
              <ArrowDown size={16} />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              aria-label="移除排序规则"
              onClick={() => setDraft({ ...draft, sort: draft.sort.filter((_, i) => i !== index) })}
            >
              <X size={16} />
            </Button>
          </div>
        ))}
        <Button
          variant="outline"
          disabled={draft.sort.length >= 3}
          onClick={() => {
            const field = (['name', 'entries', 'created'] as const).find(
              (field) => !draft.sort.some((rule) => rule.field === field),
            )
            if (field) setDraft({ ...draft, sort: [...draft.sort, { field, direction: 'asc' }] })
          }}
        >
          <Plus size={16} />
          添加排序规则
        </Button>
      </div>
      <ErrorNotice error={error} />
      <SaveStatus pending={autosave.pending} error={autosave.error} retry={autosave.flush} />
      {!autosave.valid && <p role="alert" className="text-sm text-destructive">请填写分组名称。</p>}
    </div>
  )
}
export default function GroupsManager({
  groups,
  dictionaries,
}: {
  groups: Group[]
  dictionaries: Dictionary[]
}) {
  const [name, setName] = useState('')
  const [creating, setCreating] = useState(false)
  const { pending, error, run } = useAction()
  return (
    <div className="space-y-4">
      <Dialog open={creating} onOpenChange={(open) => { if (!pending) { setCreating(open); if (!open) setName('') } }}>
        <DialogTrigger asChild>
          <Button variant="ghost" size="icon" aria-label="创建分组" title="创建分组"><Plus className="size-4" /></Button>
        </DialogTrigger>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>创建分组</DialogTitle>
            <DialogDescription className="sr-only">输入新分组的名称</DialogDescription>
          </DialogHeader>
          <form className="space-y-4" onSubmit={async (event) => {
            event.preventDefault()
            if (pending || !name.trim()) return
            const result = await run(() => api<Group>('/groups', {
              method: 'POST', body: JSON.stringify({ name: name.trim(), dictionary_ids: [], sort: [] }),
            }), '分组已创建')
            if (result) { setName(''); setCreating(false) }
          }}>
            <Input autoFocus aria-label="新分组名称" placeholder="分组名称" value={name}
              onChange={(event) => setName(event.target.value)} />
            <ErrorNotice error={error} />
            <DialogFooter>
              <Button type="button" variant="ghost" disabled={pending} onClick={() => { setCreating(false); setName('') }}>取消</Button>
              <Button type="submit" disabled={pending || !name.trim()}>{pending ? '创建中…' : '创建'}</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      {!groups.length && <Empty>暂无分组</Empty>}
      {groups.map((group) => (
        <GroupEditor
          key={group.id}
          group={group}
          dictionaries={dictionaries}
        />
      ))}
    </div>
  )
}
