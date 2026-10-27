import { useEffect, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
export function useAction() {
  const client = useQueryClient()
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  async function run<T>(action: () => Promise<T>, message = '已保存'): Promise<T | undefined> {
    setPending(true)
    setError('')
    try {
      const value = await action()
      await client.invalidateQueries({ queryKey: ['bootstrap'] })
      if (message) toast.success(message)
      return value
    } catch (cause) {
      const text = errorMessage(cause)
      setError(text)
      toast.error(text)
      return undefined
    } finally {
      setPending(false)
    }
  }
  return { pending, error, run }
}
export function Section({
  title,
  description,
  children,
}: {
  title: string
  description?: string
  children: ReactNode
}) {
  return (
    <section className="settings-section">
      <div>
        <h3 className="settings-section-title">{title}</h3>
        {description && (
          <p className="mt-1 max-w-prose text-xs leading-5 text-muted-foreground">{description}</p>
        )}
      </div>
      {children}
    </section>
  )
}
export function Field({
  label,
  description,
  children,
  htmlFor,
  className = '',
}: {
  label: string
  description?: string
  children: ReactNode
  htmlFor?: string
  className?: string
}) {
  return (
    <div className={`settings-field ${className}`}>
      <div>
        <Label htmlFor={htmlFor} className="leading-6">
          {label}
        </Label>
        {description && (
          <p className="max-w-prose text-xs leading-5 text-muted-foreground">{description}</p>
        )}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  )
}
export function Toggle({
  label,
  description,
  checked,
  onChange,
  disabled,
}: {
  label: string
  description?: string
  checked: boolean
  onChange: (value: boolean) => void
  disabled?: boolean
}) {
  return (
    <Field label={label} description={description} className="settings-toggle">
      <div className="flex min-h-9 items-center justify-end">
        <Switch
          aria-label={label}
          checked={checked}
          onCheckedChange={onChange}
          disabled={disabled}
        />
      </div>
    </Field>
  )
}
export function Choice({
  label,
  value,
  items,
  onChange,
  disabled,
}: {
  label: string
  value: string
  items: { value: string; label: string }[]
  onChange: (value: string) => void
  disabled?: boolean
}) {
  return (
    <Select value={value} onValueChange={onChange} disabled={disabled}>
      <SelectTrigger aria-label={label} className="w-full">
        <SelectValue placeholder="请选择" />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.value} value={item.value}>
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
export function NumberField({
  label,
  value,
  onChange,
  min,
  max,
  step = 1,
  description,
}: {
  label: string
  value: number
  onChange: (value: number) => void
  min: number
  max: number
  step?: number
  description?: string
}) {
  const [text, setText] = useState(String(value))
  useEffect(() => setText(String(value)), [value])
  return (
    <Field label={label} description={description}>
      <Input
        aria-label={label}
        type="number"
        value={text}
        min={min}
        max={max}
        step={step}
        onChange={(event) => {
          setText(event.target.value)
          const n = event.target.valueAsNumber
          if (Number.isFinite(n)) onChange(n)
        }}
        onBlur={() => { if (!text.trim()) setText(String(value)) }}
      />
    </Field>
  )
}
export function Confirm({
  title,
  description,
  label,
  onConfirm,
  disabled,
  destructive = false,
}: {
  title: string
  description: string
  label: string
  onConfirm: () => Promise<unknown>
  disabled?: boolean
  destructive?: boolean
}) {
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  return (
    <>
      <Button
        variant="outline"
        className={destructive ? 'text-destructive hover:text-destructive' : undefined}
        disabled={disabled}
        onClick={() => setOpen(true)}
      >
        {label}
      </Button>
      <AlertDialog open={open} onOpenChange={setOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{title}</AlertDialogTitle>
            <AlertDialogDescription>{description}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={busy}
              onClick={async (event) => {
                event.preventDefault()
                setBusy(true)
                try {
                  await onConfirm()
                  setOpen(false)
                } finally {
                  setBusy(false)
                }
              }}
            >
              {busy ? '处理中…' : '确认'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
export function ErrorNotice({ error }: { error: string }) {
  return error ? (
    <p role="alert" className="break-words text-sm text-destructive">
      {error}
    </p>
  ) : null
}
export function Empty({ children }: { children: ReactNode }) {
  return <p className="py-8 text-sm leading-6 text-muted-foreground">{children}</p>
}
export function bytes(size: number) {
  return size < 1024
    ? `${size} B`
    : size < 1048576
      ? `${(size / 1024).toFixed(1)} KB`
      : `${(size / 1048576).toFixed(1)} MB`
}
export function FileTree({ files }: { files: { path: string; size: number }[] }) {
  const roots = new Map<string, { path: string; size: number }[]>()
  for (const file of files) {
    const slash = file.path.lastIndexOf('/')
    const parent = slash >= 0 ? file.path.slice(0, slash) : '根目录'
    roots.set(parent, [...(roots.get(parent) ?? []), file])
  }
  return (
    <ScrollArea className="max-h-80 border-y py-3 text-sm" aria-label="文件目录树">
      {[...roots]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([folder, children]) => (
          <div key={folder} className="mb-3">
            <p className="break-all font-medium">{folder}/</p>
            <ul className="ml-4 border-l pl-3">
              {children.map((file) => (
                <li key={file.path} className="flex gap-3 py-1">
                  <span className="min-w-0 flex-1 break-all">{file.path.split('/').at(-1)}</span>
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {bytes(file.size)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        ))}
    </ScrollArea>
  )
}

export function SaveStatus({ pending, error, retry }: {
  pending: boolean
  error: string
  retry: () => Promise<unknown>
}) {
  if (error) return (
    <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-destructive">
      <span>{error}</span>
      <Button variant="outline" size="sm" onClick={() => void retry()}>重试</Button>
    </div>
  )
  return pending ? <span role="status" className="text-xs text-muted-foreground">保存中…</span> : null
}
