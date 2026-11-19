import {
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type InputHTMLAttributes,
  type Ref,
} from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FileText,
  FolderOpen,
  FolderPlus,
  Plus,
  Server,
  Upload,
  X,
} from 'lucide-react'
import { toast } from 'sonner'
import { api, apiUpload, ApiError, getConnection, getDesktopBridge, type UploadProgress } from '@/lib/api'
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

interface StagedFile {
  file: File
  relativePath: string
}

interface PendingBatch {
  id: string
  label: string
  isFolder: boolean
  files: StagedFile[]
  mdxCount: number
  totalBytes: number
}

interface FileSystemEntryLike {
  isFile: boolean
  isDirectory: boolean
  name: string
  file?: (success: (file: File) => void, error?: (err: unknown) => void) => void
  createReader?: () => {
    readEntries: (success: (entries: FileSystemEntryLike[]) => void, error?: (err: unknown) => void) => void
  }
}

async function readDirectoryEntry(entry: FileSystemEntryLike, prefix: string): Promise<StagedFile[]> {
  if (entry.isFile && entry.file) {
    const file = await new Promise<File>((resolve, reject) => {
      entry.file!(resolve, reject)
    })
    return [{ file, relativePath: prefix ? `${prefix}/${entry.name}` : entry.name }]
  }
  if (entry.isDirectory && entry.createReader) {
    const reader = entry.createReader()
    const entries: FileSystemEntryLike[] = []
    while (true) {
      const chunk = await new Promise<FileSystemEntryLike[]>((resolve, reject) => {
        reader.readEntries(resolve, reject)
      })
      if (!chunk.length) break
      entries.push(...chunk)
    }
    const nextPrefix = prefix ? `${prefix}/${entry.name}` : entry.name
    const nested = await Promise.all(entries.map((child) => readDirectoryEntry(child, nextPrefix)))
    return nested.flat()
  }
  return []
}

async function extractDroppedBatches(dataTransfer: DataTransfer): Promise<PendingBatch[]> {
  const items = Array.from(dataTransfer.items ?? [])
  const withEntry = items
    .map((item) => {
      const getter = (item as DataTransferItem & { webkitGetAsEntry?: () => unknown }).webkitGetAsEntry
      return (getter?.call(item) as FileSystemEntryLike | null | undefined) ?? null
    })
    .filter((entry): entry is FileSystemEntryLike => entry !== null)

  if (withEntry.length > 0) {
    const batches: PendingBatch[] = []
    const looseFiles: StagedFile[] = []
    for (const entry of withEntry) {
      if (entry.isDirectory) {
        const files = await readDirectoryEntry(entry, '')
        if (files.length > 0) {
          batches.push(makeBatch(entry.name, true, files))
        }
      } else if (entry.isFile) {
        const files = await readDirectoryEntry(entry, '')
        looseFiles.push(...files)
      }
    }
    if (looseFiles.length > 0) {
      batches.push(makeBatch(looseFiles.length === 1 ? looseFiles[0].file.name : `${looseFiles.length} 个散落文件`, false, looseFiles))
    }
    return batches
  }

  const files = Array.from(dataTransfer.files ?? []).filter((file) => file.size > 0 || file.name.includes('.'))
  if (!files.length) return []
  return groupFilesToBatches(files)
}

function makeBatch(label: string, isFolder: boolean, files: StagedFile[]): PendingBatch {
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    label,
    isFolder,
    files,
    mdxCount: files.filter((item) => item.relativePath.toLowerCase().endsWith('.mdx')).length,
    totalBytes: files.reduce((sum, item) => sum + item.file.size, 0),
  }
}

function groupFilesToBatches(fileList: File[] | FileList): PendingBatch[] {
  const files = Array.from(fileList)
  if (!files.length) return []
  const byFolder = new Map<string, StagedFile[]>()
  const loose: StagedFile[] = []
  for (const file of files) {
    const rel = file.webkitRelativePath || ''
    if (rel.includes('/')) {
      const top = rel.split('/')[0]
      const list = byFolder.get(top) ?? []
      list.push({ file, relativePath: rel })
      byFolder.set(top, list)
    } else {
      loose.push({ file, relativePath: file.name })
    }
  }
  const batches: PendingBatch[] = []
  for (const [folder, list] of byFolder.entries()) {
    batches.push(makeBatch(folder, true, list))
  }
  if (loose.length > 0) {
    batches.push(makeBatch(loose.length === 1 ? loose[0].file.name : `${loose.length} 个词典文件`, false, loose))
  }
  return batches
}

function baseName(rawPath: string) {
  const trimmed = rawPath.replace(/[\\/]+$/, '')
  return trimmed.split(/[\\/]/).at(-1) || trimmed
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
      { label: '读取/传输文件', state: 'active' },
      { label: '识别关联资源', state: 'pending' },
      { label: '解析词头', state: 'pending' },
      { label: '更新检索目录', state: 'pending' },
    ]
  }
  if (phase === 'inspecting') {
    return [
      { label: '读取/传输文件', state: 'done' },
      { label: '识别关联资源', state: 'active' },
      { label: '解析词头', state: 'pending' },
      { label: '更新检索目录', state: 'pending' },
    ]
  }
  const activeIdx = stage === 'catalog' ? 2 : stage === 'headwords' ? 1 : 0
  return [
    { label: '文件准备', state: 'done' },
    { label: '整理文件', state: activeIdx > 0 ? 'done' : 'active' },
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
  const desktopBridge = getDesktopBridge()
  const isDesktop = Boolean(desktopBridge?.local_only)
  const canUseNativeDialog = isDesktop && Boolean(desktopBridge?.pickImportFolders)

  const [source, setSource] = useState<ImportSource>('files')
  const [pendingLocalPaths, setPendingLocalPaths] = useState<string[]>([])
  const [pendingBatches, setPendingBatches] = useState<PendingBatch[]>([])
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [selectedMdx, setSelectedMdx] = useState<string[]>([])
  const [hostPath, setHostPath] = useState('')
  const [dragging, setDragging] = useState(false)
  const [lastImportedCount, setLastImportedCount] = useState(0)
  const [expandedResources, setExpandedResources] = useState<Record<string, boolean>>({})
  const [expandedOtherDirs, setExpandedOtherDirs] = useState<Record<string, boolean>>({})
  const uploadRef = useRef<HTMLInputElement>(null)
  const directoryRef = useRef<HTMLInputElement>(null)
  const { pending, error, run } = useAction()
  const client = useQueryClient()
  const storageKey = `fastdict.import:${isDesktop ? 'desktop-local' : getConnection().base_url}`
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
      : source === 'server' || isDesktop
        ? '正在扫描并读取词典目录'
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
    setPendingLocalPaths([])
    setPendingBatches([])
    setExpandedOtherDirs({})
    // Expand companion resources automatically only for candidates that have warnings.
    const autoOpen: Record<string, boolean> = {}
    for (const candidate of next.candidates) {
      if (candidate.warnings.length > 0) autoOpen[candidate.mdx] = true
    }
    setExpandedResources(autoOpen)
    const fresh = next.candidates
      .filter((item) => !existingNames.has(item.name.trim().toLowerCase()))
      .map((item) => item.mdx)
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
    open(next, pickImmediately = false) {
      onStart()
      if (busy) return
      forgetJob()
      setSource(next)
      if (next === 'server') setPreview(null)
      if (pickImmediately) {
        if (next === 'directory' && canUseNativeDialog) void pickNativeFolders()
        else if (next === 'files' && canUseNativeDialog) void pickNativeFiles()
        else if (next === 'files') uploadRef.current?.click()
        else if (next === 'directory') directoryRef.current?.click()
      }
    },
  }))

  async function pickNativeFolders() {
    const picker = desktopBridge?.pickImportFolders
    if (!picker || busy) return
    try {
      const picked = await picker()
      if (!picked.length) return
      setPendingLocalPaths((current) => {
        const next = [...current]
        for (const p of picked) {
          if (!next.includes(p)) next.push(p)
        }
        return next
      })
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : '打开文件夹选择器失败')
    }
  }

  async function pickNativeFiles() {
    const picker = desktopBridge?.pickImportFiles
    if (!picker || busy) return
    try {
      const picked = await picker()
      if (!picked.length) return
      setPendingLocalPaths((current) => {
        const next = [...current]
        for (const p of picked) {
          if (!next.includes(p)) next.push(p)
        }
        return next
      })
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : '打开文件选择器失败')
    }
  }

  async function scanLocalPaths(paths: string[]) {
    if (!paths.length || busy) return
    setPreview(null)
    setSelectedMdx([])
    setUploadDetail(
      paths.length === 1
        ? `正在读取：${baseName(paths[0])}`
        : `正在读取 ${paths.length} 个目录/路径：${paths.slice(0, 3).map(baseName).join('、')}${paths.length > 3 ? '…' : ''}`,
    )
    const result = await run(
      () =>
        api<ImportPreview>('/imports/path', {
          method: 'POST',
          body: JSON.stringify({ paths }),
        }),
      '',
      { refresh: false },
    )
    setUploadDetail('')
    if (result) applyPreview(result)
  }

  function appendFileList(fileList: File[] | FileList | null) {
    if (!fileList || !fileList.length || busy) return
    const nextBatches = groupFilesToBatches(fileList)
    if (!nextBatches.length) return
    setPendingBatches((current) => [...current, ...nextBatches])
  }

  async function uploadStagedBatches(batches: PendingBatch[]) {
    if (!batches.length || busy) return
    setPreview(null)
    setSelectedMdx([])
    setUploadProgress(null)

    const usedTopFolders = new Set<string>()
    const body = new FormData()
    let totalFiles = 0
    let totalBytes = 0

    for (const batch of batches) {
      let folderPrefix = ''
      if (batch.isFolder) {
        let candidate = batch.label
        let suffix = 2
        while (usedTopFolders.has(candidate.toLowerCase())) {
          candidate = `${batch.label}-${suffix}`
          suffix += 1
        }
        usedTopFolders.add(candidate.toLowerCase())
        folderPrefix = candidate
      }

      for (const item of batch.files) {
        let rel = item.relativePath.replace(/^\/+/, '')
        if (batch.isFolder && folderPrefix && rel.includes('/')) {
          const rest = rel.slice(rel.indexOf('/') + 1)
          rel = `${folderPrefix}/${rest}`
        } else if (batch.isFolder && folderPrefix) {
          rel = `${folderPrefix}/${rel}`
        }
        body.append('files', item.file, rel)
        totalFiles += 1
        totalBytes += item.file.size
      }
    }

    setUploadDetail(`${batches.length} 组来源 · 共 ${totalFiles} 个文件 (${bytes(totalBytes)})`)
    onStart()
    const result = await run(
      () => apiUpload<ImportPreview>('/imports', body, setUploadProgress),
      '',
      { refresh: false },
    )
    if (result) applyPreview(result)
    setUploadProgress(null)
    setUploadDetail('')
  }

  async function handleDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault()
    setDragging(false)
    if (busy) return
    const dropped = await extractDroppedBatches(event.dataTransfer)
    if (!dropped.length) {
      toast.error('未检测到可读取的词典文件，请使用下方按钮选择文件夹')
      return
    }
    setPendingBatches((current) => [...current, ...dropped])
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

  const totalPendingMdx = pendingBatches.reduce((sum, item) => sum + item.mdxCount, 0)
  const totalPendingBytes = pendingBatches.reduce((sum, item) => sum + item.totalBytes, 0)

  return (
    <Section
      title="导入词典"
      description={
        isDesktop
          ? '支持一次多选或连续追加多个词典文件夹，直接从本机磁盘高速读取并自动配对 .mdd、.css、.js 与封面图标。'
          : '支持同时拖入多个词典文件夹，或连续添加多个目录后一次性上传识别，系统会自动按目录配对伴随资源。'
      }
    >
      <input
        hidden
        ref={uploadRef}
        type="file"
        multiple
        onChange={(event) => {
          appendFileList(event.target.files)
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
          appendFileList(event.target.files)
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
            onDrop={(event) => void handleDrop(event)}
            className={`rounded-xl border-2 border-dashed p-6 text-center transition-colors ${
              dragging ? 'border-primary bg-accent/60' : 'border-border bg-muted/20 hover:bg-muted/35'
            }`}
          >
            <FolderPlus className="mx-auto size-7 text-muted-foreground" />
            <p className="mt-2.5 text-sm font-medium">
              {canUseNativeDialog
                ? '选择一个或多个词典文件夹，或直接拖入多个目录'
                : '将多个词典文件夹拖放到此处，或点击下方连续添加'}
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              {canUseNativeDialog
                ? '可按住 Ctrl / Shift 多选目录，也可分多次追加不同位置的文件夹后再统一扫描'
                : '支持追加多个文件夹一起识别；超过 16 MiB 自动分块传输（最高 50 GiB）'}
            </p>
            <div className="mt-4 flex flex-wrap items-center justify-center gap-2.5">
              {canUseNativeDialog ? (
                <>
                  <Button variant="default" size="sm" onClick={() => void pickNativeFolders()}>
                    <FolderPlus className="size-4" />
                    选择词典文件夹（可多选）
                  </Button>
                  <Button variant="outline" size="sm" onClick={() => void pickNativeFiles()}>
                    <Upload className="size-4" />
                    选择词典文件
                  </Button>
                </>
              ) : (
                <>
                  <Button variant="default" size="sm" onClick={() => directoryRef.current?.click()}>
                    <FolderPlus className="size-4" />
                    添加词典文件夹
                  </Button>
                  <Button variant="outline" size="sm" onClick={() => uploadRef.current?.click()}>
                    <Upload className="size-4" />
                    添加词典文件
                  </Button>
                  <Button
                    variant={source === 'server' ? 'secondary' : 'ghost'}
                    size="sm"
                    onClick={() => setSource(source === 'server' ? 'files' : 'server')}
                  >
                    <Server className="size-4" />
                    扫描服务端目录
                  </Button>
                </>
              )}
            </div>
          </div>

          {pendingLocalPaths.length > 0 && (
            <div className="space-y-3 rounded-xl border bg-background p-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-sm font-medium">
                  待扫描目录 / 文件（已选 {pendingLocalPaths.length} 项）
                </span>
                <div className="flex items-center gap-1.5">
                  {canUseNativeDialog && (
                    <Button variant="outline" size="sm" className="h-7 px-2.5 text-xs" onClick={() => void pickNativeFolders()}>
                      <Plus className="size-3.5" />
                      继续添加文件夹
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-xs text-muted-foreground"
                    onClick={() => setPendingLocalPaths([])}
                  >
                    清空
                  </Button>
                </div>
              </div>
              <ul className="divide-y overflow-hidden rounded-lg border">
                {pendingLocalPaths.map((itemPath) => (
                  <li key={itemPath} className="flex items-center justify-between gap-3 px-3 py-2 text-xs">
                    <span className="flex min-w-0 items-center gap-2">
                      <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
                      <span className="min-w-0">
                        <span className="block truncate font-medium text-foreground">{baseName(itemPath)}</span>
                        <span className="block truncate text-muted-foreground">{itemPath}</span>
                      </span>
                    </span>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7 shrink-0 text-muted-foreground hover:text-destructive"
                      aria-label={`移除 ${baseName(itemPath)}`}
                      onClick={() => setPendingLocalPaths((current) => current.filter((p) => p !== itemPath))}
                    >
                      <X className="size-3.5" />
                    </Button>
                  </li>
                ))}
              </ul>
              <div className="flex justify-end">
                <Button size="sm" onClick={() => void scanLocalPaths(pendingLocalPaths)}>
                  开始扫描并识别（{pendingLocalPaths.length} 项）
                </Button>
              </div>
            </div>
          )}

          {pendingBatches.length > 0 && (
            <div className="space-y-3 rounded-xl border bg-background p-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-sm font-medium">
                  待上传清单（{pendingBatches.length} 组 · 发现 {totalPendingMdx} 本词典 · 共 {bytes(totalPendingBytes)}）
                </span>
                <div className="flex items-center gap-1.5">
                  <Button variant="outline" size="sm" className="h-7 px-2.5 text-xs" onClick={() => directoryRef.current?.click()}>
                    <Plus className="size-3.5" />
                    继续添加文件夹
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-xs text-muted-foreground"
                    onClick={() => setPendingBatches([])}
                  >
                    清空
                  </Button>
                </div>
              </div>
              <ul className="divide-y overflow-hidden rounded-lg border">
                {pendingBatches.map((batch) => (
                  <li key={batch.id} className="flex items-center justify-between gap-3 px-3 py-2 text-xs">
                    <span className="flex min-w-0 items-center gap-2">
                      {batch.isFolder ? (
                        <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
                      ) : (
                        <FileText className="size-3.5 shrink-0 text-muted-foreground" />
                      )}
                      <span className="min-w-0">
                        <span className="block truncate font-medium text-foreground">{batch.label}</span>
                        <span className="block text-muted-foreground">
                          {batch.mdxCount} 本 .mdx · {batch.files.length} 个文件 · {bytes(batch.totalBytes)}
                        </span>
                      </span>
                    </span>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7 shrink-0 text-muted-foreground hover:text-destructive"
                      aria-label={`移除 ${batch.label}`}
                      onClick={() => setPendingBatches((current) => current.filter((item) => item.id !== batch.id))}
                    >
                      <X className="size-3.5" />
                    </Button>
                  </li>
                ))}
              </ul>
              <div className="flex justify-end">
                <Button size="sm" onClick={() => void uploadStagedBatches(pendingBatches)}>
                  上传并识别全部（{totalPendingMdx || pendingBatches.length}）
                </Button>
              </div>
            </div>
          )}

          {!canUseNativeDialog && source === 'server' && !jobId && (
            <div className="space-y-2.5 rounded-xl border p-4">
              <p className="text-xs text-muted-foreground">
                输入服务端授权目录路径（支持每行一个，一次扫描多个目录）：
              </p>
              <textarea
                aria-label="服务主机上的授权目录路径"
                rows={3}
                className="w-full rounded-md border bg-background px-3 py-2 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                placeholder={'/data/dictionaries/OALD\n/data/dictionaries/LDOCE'}
                value={hostPath}
                onChange={(event) => setHostPath(event.target.value)}
              />
              <div className="flex justify-end">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy || !hostPath.trim()}
                  onClick={async () => {
                    const paths = hostPath
                      .split(/\r?\n/)
                      .map((line) => line.trim())
                      .filter(Boolean)
                    await scanLocalPaths(paths)
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
        <fieldset disabled={busy} className="min-w-0 space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border bg-muted/25 px-4 py-3">
            <div className="flex items-center gap-2 text-sm font-medium">
              <CheckCircle2 className="size-4 text-emerald-600 dark:text-emerald-400" />
              <span>
                共识别 {preview.candidates.length} 本词典（{preview.files.length} 个文件），已选{' '}
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
                所选目录中没有发现 <code className="rounded bg-muted px-1">.mdx</code> 主词典文件（若只需为已有词典补充 MDD/CSS/字体，请在左侧选中对应词典后点击“添加资源”）。
              </p>
              {preview.files.length > 0 && <FileTree files={preview.files} />}
            </div>
          )}

          <div className="space-y-3">
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
              const resourcesOpen = !!expandedResources[candidate.mdx]
              const showAllDirs = !!expandedOtherDirs[candidate.mdx]
              const visibleCompanions = (
                showAllDirs || scopedCompanions.length === 0 ? allCompanionFiles : scopedCompanions
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
                  className={`space-y-3 rounded-xl border p-4 transition-opacity ${
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
                    <div className="min-w-0 flex-1 space-y-2">
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
                        <div className="pt-0.5">
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <button
                              type="button"
                              className="inline-flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground"
                              onClick={() =>
                                setExpandedResources((prev) => ({
                                  ...prev,
                                  [candidate.mdx]: !resourcesOpen,
                                }))
                              }
                            >
                              {resourcesOpen ? (
                                <ChevronDown className="size-3.5" />
                              ) : (
                                <ChevronRight className="size-3.5" />
                              )}
                              已自动关联 {candidate.resources.length} 个伴随资源（点击{resourcesOpen ? '收起' : '调整'}）
                            </button>
                            {resourcesOpen && hiddenCount > 0 && scopedCompanions.length > 0 && (
                              <button
                                type="button"
                                className="text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
                                onClick={() =>
                                  setExpandedOtherDirs((prev) => ({
                                    ...prev,
                                    [candidate.mdx]: !showAllDirs,
                                  }))
                                }
                              >
                                {showAllDirs ? '仅显示本目录资源' : `显示其他目录文件 (${hiddenCount})`}
                              </button>
                            )}
                          </div>

                          {resourcesOpen && (
                            <ScrollArea
                              className="mt-2 max-h-44 rounded-lg border bg-muted/15 p-2.5"
                              viewportClassName="space-y-1.5"
                            >
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
                          )}
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
              重新选择目录
            </Button>
          </div>
        </fieldset>
      )}
    </Section>
  )
}
