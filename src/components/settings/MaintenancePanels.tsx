import { useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Download, RefreshCw, Upload } from 'lucide-react'
import { api, apiBlob } from '@/lib/api'
import type { LogItem } from '@/lib/types'
import { Button } from '@/components/ui/button'
import {
  Choice,
  Confirm,
  Empty,
  ErrorNotice,
  Section,
  Toggle,
  bytes,
  errorMessage,
  useAction,
} from './shared'

export function LogsPanel() {
  const client = useQueryClient()
  const [limit, setLimit] = useState('200')
  const { pending, error, run } = useAction()
  const logs = useQuery({
    queryKey: ['logs', limit],
    queryFn: ({ signal }) => api<{ items: LogItem[] }>(`/logs?limit=${limit}`, { signal }),
  })
  return (
    <Section
      title="服务日志"

    >
      <div className="flex flex-wrap gap-2">
        <div className="w-40">
          <Choice
            label="日志条数"
            value={limit}
            onChange={setLimit}
            items={[
              { value: '50', label: '最近 50 条' },
              { value: '200', label: '最近 200 条' },
              { value: '1000', label: '最近 1000 条' },
            ]}
          />
        </div>
        <Button variant="outline" disabled={logs.isFetching} onClick={() => void logs.refetch()}>
          <RefreshCw size={16} />
          刷新
        </Button>
        <Confirm
          title="清空服务日志？"
          description="当前服务保存的诊断日志将被删除，无法撤销。不影响词典、历史或配置。"
          label="清空日志"
          destructive
          disabled={pending || !logs.data?.items.length}
          onConfirm={() =>
            run(async () => {
              await api('/logs', { method: 'DELETE' })
              await client.invalidateQueries({ queryKey: ['logs'] })
            }, '日志已清空')
          }
        />
      </div>
      <ErrorNotice error={logs.error ? errorMessage(logs.error) : error} />
      {logs.isPending && <p role="status">正在读取日志…</p>}
      {logs.data?.items.length === 0 && <Empty>暂无日志</Empty>}
      <ol className="divide-y">
        {logs.data?.items.map((log) => (
          <li key={log.id} className="space-y-2 py-4">
            <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
              <time dateTime={log.at}>
                {Number.isNaN(Date.parse(log.at)) ? log.at : new Date(log.at).toLocaleString()}
              </time>
              <span>{log.level}</span>
              <span>{log.scope}</span>
            </div>
            <p className="whitespace-pre-wrap break-all text-sm leading-6">{log.message}</p>
          </li>
        ))}
      </ol>
    </Section>
  )
}
export function BackupPanel() {
  const [includeFiles, setIncludeFiles] = useState(false)
  const [file, setFile] = useState<File | null>(null)
  const input = useRef<HTMLInputElement>(null)
  const { pending, error, run } = useAction()
  async function backup() {
    await run(async () => {
      const blob = await apiBlob(`/backup?include_files=${includeFiles}`)
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `fastdict-backup-${new Date().toISOString().slice(0, 10)}.${includeFiles ? 'zip' : 'json'}`
      anchor.click()
      setTimeout(() => URL.revokeObjectURL(url), 60_000)
    }, '备份已开始下载')
  }
  return (
    <div className="space-y-10">
      <Section
        title="导出备份"
        description="跨设备恢复时，请包含词典源文件。"
      >
        <Toggle
          label="包含词典源文件"

          checked={includeFiles}
          onChange={setIncludeFiles}
          disabled={pending}
        />
        <Button disabled={pending} onClick={() => void backup()}>
          <Download size={16} />
          {pending ? '处理中…' : '下载备份'}
        </Button>
      </Section>
      <Section
        title="从备份恢复"
        description="恢复会替换当前配置与数据。"
      >
        <input
          hidden
          ref={input}
          type="file"
          accept=".zip,.json,application/zip,application/json"
          onChange={(event) => {
            setFile(event.target.files?.[0] ?? null)
            event.target.value = ''
          }}
        />
        <Button variant="outline" disabled={pending} onClick={() => input.current?.click()}>
          <Upload size={16} />
          选择备份文件
        </Button>
        {file && (
          <div className="space-y-3">
            <p className="break-all text-sm">
              {file.name} · {bytes(file.size)}
            </p>
            <Confirm
              title="用此备份替换当前服务数据？"
              description={`将恢复“${file.name}”。当前配置、分组及历史可能被替换，此操作无法直接撤销。所有词典脚本授权都会重置。`}
              label="恢复此备份"
              destructive
              disabled={pending}
              onConfirm={() =>
                run(async () => {
                  const body = new FormData()
                  body.append('file', file)
                  await api('/restore', { method: 'POST', body })
                  setFile(null)
                }, '备份已恢复，脚本保持禁用')
              }
            />
          </div>
        )}
      </Section>
      <ErrorNotice error={error} />
    </div>
  )
}
