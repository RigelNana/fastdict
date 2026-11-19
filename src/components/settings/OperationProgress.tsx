import { Check, LoaderCircle } from 'lucide-react'
import type { UploadProgress } from '@/lib/api'
import { bytes } from './shared'

export interface ProgressStep {
  label: string
  state: 'done' | 'active' | 'pending'
}

export default function OperationProgress({
  label,
  detail,
  upload,
  percent: explicitPercent,
  steps,
}: {
  label: string
  detail?: string
  upload?: UploadProgress | null
  percent?: number
  steps?: ProgressStep[]
}) {
  const transferring = upload?.phase === 'uploading'
  const computedPercent =
    explicitPercent !== undefined
      ? Math.min(100, Math.max(0, explicitPercent))
      : transferring && upload.total > 0
        ? Math.min(100, (upload.loaded / upload.total) * 100)
        : undefined

  return (
    <div className="operation-progress space-y-3 rounded-xl border bg-muted/25 p-4">
      {steps && steps.length > 0 && (
        <ol className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs" aria-label="处理阶段">
          {steps.map((step, index) => (
            <li
              key={step.label}
              className={
                step.state === 'active'
                  ? 'flex items-center gap-1.5 font-semibold text-foreground'
                  : step.state === 'done'
                    ? 'flex items-center gap-1.5 text-emerald-600 dark:text-emerald-400'
                    : 'flex items-center gap-1.5 text-muted-foreground/70'
              }
            >
              {step.state === 'done' ? (
                <Check className="size-3.5 shrink-0" />
              ) : step.state === 'active' ? (
                <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
              ) : (
                <span className="flex size-4 shrink-0 items-center justify-center rounded-full border text-[10px] tabular-nums">
                  {index + 1}
                </span>
              )}
              <span>{step.label}</span>
            </li>
          ))}
        </ol>
      )}

      <div className="flex items-center justify-between gap-4 text-sm">
        <span role="status" className="font-medium">
          {upload?.retry ? '网络波动，正在自动重试分块…' : transferring ? '正在上传词典文件' : label}
        </span>
        {computedPercent !== undefined && (
          <span className="text-xs font-medium tabular-nums text-muted-foreground">
            {Math.floor(computedPercent)}%
          </span>
        )}
      </div>

      <div
        role="progressbar"
        aria-label={transferring ? '文件上传进度' : label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={computedPercent === undefined ? undefined : Math.floor(computedPercent)}
        className="h-1.5 overflow-hidden rounded-full bg-muted"
      >
        <div
          className={
            computedPercent === undefined
              ? 'operation-progress-indeterminate h-full w-1/3 rounded-full bg-primary/70'
              : 'h-full rounded-full bg-primary transition-[width] duration-200 ease-out'
          }
          style={computedPercent === undefined ? undefined : { width: `${computedPercent}%` }}
        />
      </div>

      <div className="flex min-w-0 items-center justify-between gap-4 text-xs text-muted-foreground">
        <span className="min-w-0 truncate">{detail}</span>
        {transferring && upload.total > 0 && (
          <span className="shrink-0 tabular-nums">
            {bytes(upload.loaded)} / {bytes(upload.total)}
          </span>
        )}
      </div>
    </div>
  )
}
