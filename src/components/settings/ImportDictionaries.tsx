import { useEffect, useImperativeHandle, useMemo, useRef, useState, type Ref, type InputHTMLAttributes, type DragEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, FolderSearch, Server, Upload } from 'lucide-react'
import { toast } from 'sonner'
import { api, apiUpload, ApiError, getConnection, type UploadProgress } from '@/lib/api'
import type { Dictionary, ImportPreview, Job } from '@/lib/types'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Checkbox } from '@/components/ui/checkbox'
import { ScrollArea } from '@/components/ui/scroll-area'
import { ErrorNotice, FileTree, Section, bytes, useAction } from './shared'
import OperationProgress, { type ProgressStep } from './OperationProgress'

export type ImportSource = 'files' | 'directory' | 'server'
export interface ImportHandle {
  open: (source: ImportSource, pickImmediately?: boolean) => void
}

const stageTitles: Record<string, string> = {
  queued: '排队等待导入',
  preparing: '正在校验并整理词典文件',
  headwords: '正在解析 MDX 词头索引',
  catalog: '正在写入数据库并更新检索目录',
}

function computeImportPercent(job?: Job): number {
  if (!job) return 5
  const total = Math.max(1, job.total)
  switch (job.stage) {
    case 'queued':
      return 6
    case 'preparing':
      return 14
    case 'headwords':
      return Math.round(18 + (Math.min(job.progress, total) / total) * 64)
    case 'catalog':
      return 90
    case 'done':
      return 100
    default:
      return 10
  }
}

function buildImportSteps(phase: 'uploading' | 'inspecting' | 'importing', stage?: string): ProgressStep[] {
  if (phase === 'uploading') {
    return [
      { label: '上传传输', state: 'active' },
      { label: '识别关联资源', state: 'pending' },
      { label: '解析词头', state: 'pending' },
      { label: '更新检索目录', state: 'pending' },
    ]
  }
  if (phase === 'inspecting') {
    return [
      { label: '上传传输', state: 'done' },
      { label: '识别关联资源', state: 'active' },
      { label: '解析词头', state: 'pending' },
      { label: '更新检索目录', state: 'pending' },
    ]
  }
  const order = ['preparing', 'headwords', 'catalog']
  const activeIdx = stage === 'catalog' ? 2 : stage === 'headwords' ? 1 : 0
  return [
    { label: '上传与预览', state: 'done' },
    { label: '整理文件', state: activeIdx > 0 ? 'done' : order[activeIdx] === 'preparing' ? 'active' : 'active' },
    { label: '解析词头', state: activeIdx > 1 ? 'done' : activeIdx === 1 ? 'active' : 'pending' },
    { label: '更新检索目录', state: activeIdx === 2 ? 'active' : 'pending' },
  ]
}

export default function ImportDictionaries({
  ref,
  existingDictionaries = [],
  onStart,
  onImported,
  onActivityChange,
}: {
  ref: Ref<ImportHandle>
  existingDictionaries?: Dictionary[]
  onStart: () => void
  onImported: (id: string | null) => void
  onActivityChange?: (state: { busy: boolean; label: string; percent?: number } | null) => void
}) {
  const [source, setSource] = useState<ImportSource>('files')
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [selectedMdx, setSelectedMdx] = useState<string[]>([])
  const [hostPath, setHostPath] = useState('')
  const [dragging, setDragging] = useState(false)
  const [lastImportedCount, setLastImportedCount] = useState(0)
  const [expandedCompanions, setExpandedCompanions] = useState<Record<string, boolean>>({})
  const uploadRef = useRef<HTMLInputElement>(null)
  const directoryRef = useRef<HTMLInputElement>(null)
  const { pending, error, run } = useAction()
  const client = useQueryClient()
  const storageKey = `fastdict.import:${getConnection().base_url}`
  const [jobId, setJobId] = useState<string | null>(() => sessionStorage.getItem(storageKey))
  const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(null)
  const [uploadDetail, setUploadDetail] = useState('')
  const completed = useRef<string | null>(null)

  const existingNames = useMemo(
    () => new Set(existingDictionaries.map((item) => item.name.trim().toLowerCase())),
    [existingDictionaries],
  )

  const job = useQuery({
    queryKey: ['import-job', storageKey, jobId],
    queryFn: ({ signal }) => api<Job>(`/jobs/${encodeURIComponent(jobId!)}`, { signal }),
    enabled: !!jobId,
    retry: (count, err) => !(err instanceof ApiError && err.status === 404) && count < 2,
    refetchInterval: (query) =>
      query.state.error instanceof ApiError && query.state.error.status === 404
        ? false
        : !query.state.data || ['queued', 'running'].includes(query.state.data.state)
          ? 250
          : false,
  })

  const expired = job.error instanceof ApiError && job.error.status === 404
  const importing =
    !!jobId && !expired && (!job.data || ['queued', 'running'].includes(job.data.state))
  const busy = pending || importing

  const progressLabel = importing
    ? stageTitles[job.data?.stage ?? 'queued'] ?? '正在导入词典'
    : uploadProgress?.phase === 'uploading'
      ? '正在上传词典文件'
      : source === 'server'
        ? '正在扫描并暂存服务端目录'
        : uploadDetail
          ? '正在识别词典与伴随资源'
          : '正在提交导入任务'

  const importPercent = importing
    ? computeImportPercent(job.data)
    : uploadProgress?.phase === 'uploading' && uploadProgress.total > 0
      ? (uploadProgress.loaded / uploadProgress.total) * 100
      : undefined

  useEffect(() => {
    if (!onActivityChange) return
    if (busy) {
      const detail = importing && job.data?.total ? ` (${job.data.progress}/${job.data.total})` : ''
      onActivityChange({ busy: true, label: `${progressLabel}${detail}`, percent: importPercent })
    } else {
      onActivityChange(null)
    }
  }, [busy, importing, job.data?.progress, job.data?.total, progressLabel, importPercent, onActivityChange])

  function forgetJob() {
    sessionStorage.removeItem(storageKey)
    setJobId(null)
  }

  function applyPreview(next: ImportPreview) {
    setPreview(next)
    setExpandedCompanions({})
    const fresh = next.candidates
      .filter((item) => !existingNames.has(item.name.trim().toLowerCase()))
      .map((item) => item.mdx)
    // Pre-select new dictionaries; if all exist already, keep all selected so the user decides.
    setSelectedMdx(fresh.length > 0 ? fresh : next.candidates.map((item) => item.mdx))
  }

  useEffect(() => {
    if (job.data?.state !== 'done' || completed.current === job.data.id) return
    completed.current = job.data.id
    const importedCount = job.data.total || lastImportedCount || 1
    sessionStorage.removeItem(storageKey)
    setJobId(null)
    setPreview(null)
    setSelectedMdx([])
    void client
      .invalidateQueries({ queryKey: ['bootstrap'] })
      .then(() => onImported(job.data!.dictionary_id ?? null))
    toast.success(`成功导入 ${importedCount} 本词典`)
  }, [job.data, client, storageKey, onImported, lastImportedCount])

  useImperativeHandle(ref, () => ({
    open(next, pickImmediately = true) {
      onStart()
      if (busy) return
      forgetJob()
      setSource(next)
      if (next === 'server') setPreview(null)
      if (pickImmediately) {
        if (next === 'files') uploadRef.current?.click()
        else if (next === 'directory') directoryRef.current?.click()
      }
    },
  }))

  async function uploadFiles(fileList: File[] | FileList | null) {
    const files = Array.from(fileList ?? [])
    if (!files.length || busy) return
    setPreview(null)
    setSelectedMdx([])
    setUploadProgress(null)
    const totalSize = files.reduce((sum, file) => sum + file.size, 0)
    setUploadDetail(
      files.length === 1
        ? `${files[0].name} (${bytes(totalSize)})`
        : `${files.length} 个文件 · 共 ${bytes(totalSize)}`,
    )
    onStart()
    const body = new FormData()
    for (const file of files) {
      body.append('files', file, file.webkitRelativePath || file.name)
    }
    const result = await run(
      () => apiUpload<ImportPreview>('/imports', body, setUploadProgress),
      '',
      { refresh: false },
    )
    if (result) applyPreview(result)
    setUploadProgress(null)
    setUploadDetail('')
  }

  function handleDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault()
    setDragging(false)
    if (busy) return
    if (event.dataTransfer.files?.length) {
      setSource('files')
      void uploadFiles(event.dataTransfer.files)
    }
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

  function toggleCandidateSelection(mdx: string, checked: boolean) {
    setSelectedMdx((current) =>
      checked ? (current.includes(mdx) ? current : [...current, mdx]) : current.filter((item) => item !== mdx),
    )
  }

  const chosenCandidates = useMemo(
    () => (preview ? preview.candidates.filter((item) => selectedMdx.includes(item.mdx)) : []),
    [preview, selectedMdx],
  )

  return (
    <Section
      title="导入词典"
      description="支持同时选择或拖入 .mdx 词典与伴随的 .mdd、.css、.js、字体及封面图标，系统会自动识别配对关系。"
    >
      <input
        hidden
        ref={uploadRef}
        type="file"
        multiple
        onChange={(event) => {
          void uploadFiles(event.target.files)
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
          void uploadFiles(event.target.files)
          event.target.value = ''
        }}
      />

      {!preview && !busy && (
        <div className="space-y-4">
          <div
            onDragOver={(event) => {
              event.preventDefault()
              setDragging(true)
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={handleDrop}
            className={`rounded-xl border-2 border-dashed p-6 text-center transition-colors ${
              dragging ? 'border-primary bg-accent/60' : 'border-border bg-muted/20 hover:bg-muted/35'
            }`}
          >
            <Upload className="mx-auto size-7 text-muted-foreground" />
            <p className="mt-2.5 text-sm font-medium">将词典文件拖放到此处，或选择导入方式</p>
            <p className="mt-1 text-xs text-muted-foreground">
              大体积词典（支持最高 50 GiB）会自动分块传输并支持断点重试
            </p>
            <div className="mt-4 flex flex-wrap items-center justify-center gap-2.5">
              <Button variant="default" size="sm" onClick={() => uploadRef.current?.click()}>
                <Upload className="size-4" />
                选择词典与资源文件
              </Button>
              <Button variant="outline" size="sm" onClick={() => directoryRef.current?.click()}>
                <FolderSearch className="size-4" />
                扫描整个文件夹
              </Button>
              <Button
                variant={source === 'server' ? 'secondary' : 'ghost'}
                size="sm"
                onClick={() => setSource(source === 'server' ? 'files' : 'server')}
              >
                <Server className="size-4" />
                扫描服务端目录
              </Button>
            </div>
          </div>

          {source === 'server' && !jobId && (
            <div className="space-y-2.5 rounded-xl border p-4">
              <p className="text-xs text-muted-foreground">
                输入服务端启动时通过 <code className="rounded bg-muted px-1 py-0.5">--import-root</code> 授权的主机目录路径：
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
                    const target = hostPath.trim()
                    setUploadDetail(`正在读取目录：${target}`)
                    const result = await run(
                      () =>
                        api<ImportPreview>('/imports/path', {
                          method: 'POST',
                          body: JSON.stringify({ path: target }),
                        }),
                      '目录扫描完成',
                      { refresh: false },
                    )
                    setUploadDetail('')
                    if (result) applyPreview(result)
                  }}
                >
                  开始扫描
                </Button>
              </div>
            </div>
          )}
        </div>
      )}

      <ErrorNotice error={error} />
      <ErrorNotice
        error={
          expired
            ? '导入任务记录已过期，请在左侧词典列表中确认结果。'
            : job.data?.state === 'error'
              ? job.data.error ?? '导入失败，请重试。'
              : job.error?.message ?? ''
        }
      />
      {job.error && !expired && (
        <Button variant="outline" size="sm" onClick={() => void job.refetch()}>
          重新获取导入进度
        </Button>
      )}
      {jobId && !importing && job.data?.state !== 'done' && !preview && (
        <Button variant="ghost" size="sm" onClick={forgetJob}>
          返回重新导入
        </Button>
      )}

      {busy && (
        <OperationProgress
          label={progressLabel}
          detail={
            importing
              ? [
                  job.data?.detail
                    ? job.data.stage === 'headwords'
                      ? `正在处理：${job.data.detail}`
                      : job.data.detail
                    : undefined,
                  job.data?.total ? `已完成 ${job.data.progress} / ${job.data.total} 本` : undefined,
                ]
                  .filter(Boolean)
                  .join(' · ')
              : uploadDetail
          }
          upload={importing ? null : uploadProgress}
          percent={importPercent}
          steps={buildImportSteps(
            importing
              ? 'importing'
              : uploadProgress?.phase === 'uploading'
                ? 'uploading'
                : 'inspecting',
            job.data?.stage,
          )}
        />
      )}

      {preview && !importing && (
        <fieldset disabled={busy} className="min-w-0 space-y-5">
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border bg-muted/25 px-4 py-3">
            <div className="flex items-center gap-2 text-sm font-medium">
              <CheckCircle2 className="size-4 text-emerald-600 dark:text-emerald-400" />
              <span>
                识别到 {preview.candidates.length} 本词典（共 {preview.files.length} 个文件），已勾选{' '}
                {chosenCandidates.length} 本
              </span>
            </div>
            {preview.candidates.length > 1 && (
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs"
                  onClick={() => setSelectedMdx(preview.candidates.map((item) => item.mdx))}
                >
                  全选
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs"
                  onClick={() => setSelectedMdx([])}
                >
                  清空选择
                </Button>
              </div>
            )}
          </div>

          {preview.warnings.map((warning, index) => (
            <p key={index} role="status" className="flex items-center gap-2 text-sm text-amber-700 dark:text-amber-400">
              <AlertTriangle className="size-4 shrink-0" />
              {warning}
            </p>
          ))}

          {!preview.candidates.length && (
            <div className="space-y-3 rounded-xl border p-4">
              <p className="text-sm text-muted-foreground">
                所选文件中没有发现 <code className="rounded bg-muted px-1">.mdx</code> 主词典文件（若只需为已有词典补充 MDD/CSS/字体，请在左侧选中对应词典后点击“添加资源”）。
              </p>
              {preview.files.length > 0 && <FileTree files={preview.files} />}
            </div>
          )}

          <div className="space-y-4">
            {preview.candidates.map((candidate, index) => {
              const checked = selectedMdx.includes(candidate.mdx)
              const duplicate = existingNames.has(candidate.name.trim().toLowerCase())
              const allCompanionFiles = preview.files.filter(
                (file) => file.kind !== 'mdx' && !file.path.toLowerCase().endsWith('.mdx'),
              )
              const mdxDir = candidate.mdx.includes('/')
                ? candidate.mdx.slice(0, candidate.mdx.lastIndexOf('/') + 1)
                : ''
              const scopedCompanions = allCompanionFiles.filter((file) =>
                mdxDir
                  ? file.path.startsWith(mdxDir) || candidate.resources.includes(file.path)
                  : !file.path.includes('/') || candidate.resources.includes(file.path),
              )
              const showAllCompanions = !!expandedCompanions[candidate.mdx]
              const visibleCompanions = (
                showAllCompanions || scopedCompanions.length === 0 ? allCompanionFiles : scopedCompanions
              )
                .slice()
                .sort((a, b) => {
                  const aChecked = candidate.resources.includes(a.path) ? 0 : 1
                  const bChecked = candidate.resources.includes(b.path) ? 0 : 1
                  return aChecked - bChecked || a.path.localeCompare(b.path)
                })
              const hiddenCount = allCompanionFiles.length - scopedCompanions.length
              return (
                <div
                  key={candidate.mdx}
                  className={`space-y-4 rounded-xl border p-4 transition-opacity ${
                    checked ? 'bg-background' : 'bg-muted/15 opacity-65'
                  }`}
                >
                  <div className="flex items-start gap-3">
                    <Checkbox
                      className="mt-1"
                      aria-label={`选择导入 ${candidate.name}`}
                      checked={checked}
                      onCheckedChange={(next) => toggleCandidateSelection(candidate.mdx, next === true)}
                    />
                    <div className="min-w-0 flex-1 space-y-2.5">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="break-all font-mono text-xs text-muted-foreground">
                          {candidate.mdx}
                        </span>
                        {duplicate && (
                          <Badge variant="secondary" className="text-amber-700 dark:text-amber-400">
                            已存在同名词典
                          </Badge>
                        )}
                      </div>
                      <Input
                        aria-label={`${candidate.mdx} 的词典名称`}
                        value={candidate.name}
                        placeholder="词典显示名称"
                        onChange={(event) => changeCandidate(index, { name: event.target.value })}
                      />
                      {candidate.warnings.map((warning, i) => (
                        <p key={i} className="text-xs text-amber-700 dark:text-amber-400">
                          {warning}
                        </p>
                      ))}

                      {allCompanionFiles.length > 0 && (
                        <div className="pt-1">
                          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                            <p className="text-xs font-medium text-muted-foreground">
                              关联伴随资源（已选 {candidate.resources.length} 个文件）
                            </p>
                            {hiddenCount > 0 && scopedCompanions.length > 0 && (
                              <button
                                type="button"
                                className="text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
                                onClick={() =>
                                  setExpandedCompanions((prev) => ({
                                    ...prev,
                                    [candidate.mdx]: !showAllCompanions,
                                  }))
                                }
                              >
                                {showAllCompanions ? '仅显示本词典目录资源' : `显示其他目录文件 (${hiddenCount})`}
                              </button>
                            )}
                          </div>
                          <ScrollArea className="max-h-44 rounded-lg border bg-muted/15 p-2.5" viewportClassName="space-y-1.5">
                            {visibleCompanions.map((file) => (
                              <label
                                key={file.path}
                                className="flex min-h-7 cursor-pointer items-center justify-between gap-3 rounded px-1.5 py-1 text-xs hover:bg-accent/50"
                              >
                                <span className="flex min-w-0 items-center gap-2.5">
                                  <Checkbox
                                    aria-label={`关联 ${file.path} 到 ${candidate.name}`}
                                    checked={candidate.resources.includes(file.path)}
                                    onCheckedChange={(isChecked) =>
                                      changeCandidate(index, {
                                        resources:
                                          isChecked === true
                                            ? [...candidate.resources, file.path]
                                            : candidate.resources.filter((path) => path !== file.path),
                                      })
                                    }
                                  />
                                  <span className="break-all">{file.path}</span>
                                </span>
                                <span className="shrink-0 tabular-nums text-muted-foreground">
                                  {bytes(file.size)}
                                </span>
                              </label>
                            ))}
                          </ScrollArea>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              )
            })}
          </div>

          <div className="flex flex-wrap items-center gap-2.5 pt-1">
            {preview.candidates.length > 0 && (
              <Button
                disabled={
                  busy ||
                  !chosenCandidates.length ||
                  chosenCandidates.some((candidate) => !candidate.name.trim())
                }
                onClick={async () => {
                  setLastImportedCount(chosenCandidates.length)
                  await run(
                    async () => {
                      const result = await api<{ job_id: string }>(
                        `/imports/${encodeURIComponent(preview.id)}/commit?background=true`,
                        {
                          method: 'POST',
                          body: JSON.stringify({
                            dictionaries: chosenCandidates.map(({ mdx, name, resources }) => ({
                              mdx,
                              name: name.trim(),
                              resources,
                            })),
                          }),
                        },
                      )
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
                确认导入 {chosenCandidates.length} 本词典
              </Button>
            )}
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => {
                setPreview(null)
                setSelectedMdx([])
                forgetJob()
              }}
            >
              重新选择文件
            </Button>
          </div>
        </fieldset>
      )}
    </Section>
  )
}
