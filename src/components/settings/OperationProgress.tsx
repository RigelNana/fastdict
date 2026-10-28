import type { UploadProgress } from '@/lib/api'
import { bytes } from './shared'

export default function OperationProgress({ label, detail, upload }: {
  label: string
  detail?: string
  upload?: UploadProgress | null
}) {
  const transferring = upload?.phase === 'uploading'
  const percent = transferring && upload.total > 0 ? Math.min(100, upload.loaded / upload.total * 100) : undefined
  return (
    <div className="operation-progress space-y-2 py-2">
      <div className="flex items-center justify-between gap-4 text-sm">
        <span role="status">{upload?.retry ? '正在重试' : transferring ? '正在上传' : label}</span>
        {percent !== undefined && <span className="text-xs tabular-nums text-muted-foreground">{Math.floor(percent)}%</span>}
      </div>
      <div role="progressbar" aria-label={transferring ? '文件上传进度' : label}
        aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent === undefined ? undefined : Math.floor(percent)}
        className="h-1 overflow-hidden rounded-full bg-muted">
        <div className={percent === undefined ? 'operation-progress-indeterminate h-full w-1/3 rounded-full bg-foreground/40' : 'h-full rounded-full bg-foreground/60'}
          style={percent === undefined ? undefined : { width: `${percent}%` }} />
      </div>
      <div className="flex min-w-0 items-center justify-between gap-4 text-xs text-muted-foreground">
        <span className="min-w-0 truncate">{detail}</span>
        {transferring && upload.total > 0 && <span className="shrink-0 tabular-nums">{bytes(upload.loaded)} / {bytes(upload.total)}</span>}
      </div>
    </div>
  )
}
