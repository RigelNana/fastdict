import { useEffect, useImperativeHandle, useRef, useState, type Ref, type InputHTMLAttributes } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api, apiUpload, ApiError, getConnection, type UploadProgress } from '@/lib/api'
import type { ImportPreview, Job } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Checkbox } from '@/components/ui/checkbox'
import { ScrollArea } from '@/components/ui/scroll-area'
import { ErrorNotice, FileTree, Section, useAction } from './shared'
import OperationProgress from './OperationProgress'

export type ImportSource = 'files' | 'directory' | 'server'
export interface ImportHandle { open: (source: ImportSource) => void }

const stages: Record<string, string> = { queued: '等待处理', preparing: '正在整理文件', headwords: '正在读取词头', catalog: '正在更新查询目录' }

export default function ImportDictionaries({ ref, onStart, onImported }: { ref: Ref<ImportHandle>; onStart: () => void; onImported: (id: string | null) => void }) {
  const [source, setSource] = useState<ImportSource>('files')
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [hostPath, setHostPath] = useState('')
  const uploadRef = useRef<HTMLInputElement>(null)
  const directoryRef = useRef<HTMLInputElement>(null)
  const { pending, error, run } = useAction()
  const client = useQueryClient()
  const storageKey = `fastdict.import:${getConnection().base_url}`
  const [jobId, setJobId] = useState<string | null>(() => sessionStorage.getItem(storageKey))
  const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(null)
  const [uploadDetail, setUploadDetail] = useState('')
  const completed = useRef<string | null>(null)
  const job = useQuery({
    queryKey: ['import-job', storageKey, jobId],
    queryFn: ({ signal }) => api<Job>(`/jobs/${encodeURIComponent(jobId!)}`, { signal }),
    enabled: !!jobId,
    retry: (count, error) => !(error instanceof ApiError && error.status === 404) && count < 2,
    refetchInterval: (query) => query.state.error instanceof ApiError && query.state.error.status === 404 ? false : !query.state.data || ['queued', 'running'].includes(query.state.data.state) ? 800 : false,
  })
  const expired = job.error instanceof ApiError && job.error.status === 404
  const importing = !!jobId && !expired && (!job.data || ['queued', 'running'].includes(job.data.state))
  const busy = pending || importing
  function forgetJob() {
    sessionStorage.removeItem(storageKey)
    setJobId(null)
  }
  useEffect(() => {
    if (job.data?.state !== 'done' || completed.current === job.data.id) return
    completed.current = job.data.id
    sessionStorage.removeItem(storageKey)
    setJobId(null)
    setPreview(null)
    void client.invalidateQueries({ queryKey: ['bootstrap'] }).then(() => onImported(job.data!.dictionary_id ?? null))
    toast.success('词典已导入')
  }, [job.data, client, storageKey, onImported])
  useImperativeHandle(ref, () => ({
    open(next) {
      if (busy) { onStart(); return }
      forgetJob()
      setSource(next)
      if (next === 'files') uploadRef.current?.click()
      else if (next === 'directory') directoryRef.current?.click()
      else { setPreview(null); onStart() }
    },
  }))
  async function upload(files: FileList | null) {
    if (!files?.length) return
    setPreview(null)
    setUploadProgress(null)
    setUploadDetail(files.length === 1 ? files[0].name : `${files.length} 个文件`)
    onStart()
    const body = new FormData()
    for (const file of files) body.append('files', file, file.webkitRelativePath || file.name)
    const result = await run(
      () => apiUpload<ImportPreview>('/imports', body, setUploadProgress),
      '',
      { refresh: false },
    )
    if (result) setPreview(result)
    setUploadProgress(null)
    setUploadDetail('')
  }
  function changeCandidate(index: number, patch: Partial<ImportPreview['candidates'][number]>) {
    setPreview(
      (current) =>
        current && {
          ...current,
          candidates: current.candidates.map((candidate, i) =>
            i === index ? { ...candidate, ...patch } : candidate,
          ),
        },
    )
  }
  return (
    <Section
      title={source === 'server' && !preview ? '扫描服务目录' : '导入词典'}
    >
      <input
        hidden
        ref={uploadRef}
        type="file"
        multiple
        onChange={(event) => {
          void upload(event.target.files)
          event.target.value = ''
        }}
      />
      <input
        hidden
        ref={directoryRef}
        type="file"
        multiple
        {...({ webkitdirectory: '', directory: '' } as InputHTMLAttributes<HTMLInputElement>)}
        onChange={(event) => {
          void upload(event.target.files)
          event.target.value = ''
        }}
      />
      {source === 'server' && !preview && !jobId && (
        <div className="space-y-3">
            <p className="mb-3 text-sm leading-6 text-muted-foreground">
              仅支持服务主机已授权的目录。
            </p>
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                aria-label="服务主机上的授权目录路径"
                placeholder="/data/dictionaries"
                value={hostPath}
                onChange={(event) => setHostPath(event.target.value)}
              />
              <Button
                variant="outline"
                disabled={busy || !hostPath.trim()}
                onClick={async () => {
                  const result = await run(
                    () =>
                      api<ImportPreview>('/imports/path', {
                        method: 'POST',
                        body: JSON.stringify({ path: hostPath.trim() }),
                      }),
                    '目录已扫描',
                    { refresh: false },
                  )
                  if (result) setPreview(result)
                }}
              >
                扫描目录
              </Button>
            </div>
        </div>
      )}
      <ErrorNotice error={error} />
      <ErrorNotice error={expired ? '任务记录已过期，请检查词典列表。' : job.data?.state === 'error' ? job.data.error ?? '导入失败，请重试。' : job.error?.message ?? ''} />
      {job.error && !expired && <Button variant="ghost" onClick={() => void job.refetch()}>重试进度查询</Button>}
      {jobId && !importing && job.data?.state !== 'done' && !preview && <Button variant="ghost" onClick={forgetJob}>关闭</Button>}
      {busy && <OperationProgress
        label={importing ? stages[job.data?.stage ?? 'queued'] ?? '正在导入词典' : uploadDetail ? '正在识别文件' : source === 'server' ? '正在扫描目录' : '正在提交导入'}
        detail={importing ? [job.data?.detail, job.data?.total ? `${job.data.progress} / ${job.data.total} 本` : '', '后台处理中'].filter(Boolean).join(' · ') : uploadDetail}
        upload={importing ? null : uploadProgress}
      />}
      {preview && !importing && (
        <fieldset disabled={busy} className="min-w-0 space-y-5">
          {preview.candidates.length > 0 && <h3 className="settings-section-title">导入预览 · {preview.candidates.length} 本词典</h3>}
          {preview.files.length > 0 && <FileTree files={preview.files} />}
          {preview.warnings.map((warning, index) => (
            <p key={index} role="status" className="text-sm text-amber-700 dark:text-amber-400">
              {warning}
            </p>
          ))}
          {!preview.candidates.length && !preview.warnings.length && (
            <p className="text-sm text-muted-foreground">
              未找到 MDX 文件，请重新选择。
            </p>
          )}
          {preview.candidates.map((candidate, index) => (
            <div key={candidate.mdx} className="space-y-3 border-b pb-5">
              <p className="break-all text-sm text-muted-foreground">{candidate.mdx}</p>
              <Input
                aria-label={`${candidate.mdx} 的词典名称`}
                value={candidate.name}
                onChange={(event) => changeCandidate(index, { name: event.target.value })}
              />
              {candidate.warnings.map((warning, i) => (
                <p key={i} className="text-sm text-amber-700 dark:text-amber-400">
                  {warning}
                </p>
              ))}
              <p className="text-sm font-medium">关联资源（可与其他词典共享）</p>
              <ScrollArea className="max-h-56" viewportClassName="space-y-2">
                {preview.files
                  .filter(
                    (file) => file.kind !== 'mdx' && !file.path.toLowerCase().endsWith('.mdx'),
                  )
                  .map((file) => (
                    <label key={file.path} className="flex min-h-9 items-start gap-3 text-sm">
                      <Checkbox
                        aria-label={`关联 ${file.path} 到 ${candidate.name}`}
                        checked={candidate.resources.includes(file.path)}
                        onCheckedChange={(checked) =>
                          changeCandidate(index, {
                            resources:
                              checked === true
                                ? [...candidate.resources, file.path]
                                : candidate.resources.filter((path) => path !== file.path),
                          })
                        }
                      />
                      <span className="break-all">{file.path}</span>
                    </label>
                  ))}
              </ScrollArea>
            </div>
          ))}
          {preview.candidates.length > 0 && <div className="flex flex-wrap gap-2">
            <Button
              disabled={
                busy ||
                !preview.candidates.length ||
                preview.candidates.some((candidate) => !candidate.name.trim())
              }
              onClick={async () => {
                await run(
                  async () => {
                    const result = await api<{ job_id: string }>(`/imports/${encodeURIComponent(preview.id)}/commit?background=true`, {
                      method: 'POST',
                      body: JSON.stringify({
                        dictionaries: preview.candidates.map(({ mdx, name, resources }) => ({
                          mdx,
                          name: name.trim(),
                          resources,
                        })),
                      }),
                    })
                    completed.current = null
                    client.removeQueries({ queryKey: ['import-job', storageKey, result.job_id] })
                    sessionStorage.setItem(storageKey, result.job_id)
                    setJobId(result.job_id)
                    void client.invalidateQueries({ queryKey: ['bootstrap'] })
                  },
                  '',
                  { refresh: false },
                )
              }}
            >
              确认导入 {preview.candidates.length} 本词典
            </Button>
            <Button variant="ghost" disabled={busy} onClick={() => { setPreview(null); forgetJob() }}>
              取消导入
            </Button>
          </div>}
        </fieldset>
      )}
    </Section>
  )
}
