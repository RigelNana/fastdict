import { useImperativeHandle, useRef, useState, type Ref, type InputHTMLAttributes } from 'react'
import { api } from '@/lib/api'
import type { ImportPreview } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Checkbox } from '@/components/ui/checkbox'
import { ScrollArea } from '@/components/ui/scroll-area'
import { ErrorNotice, FileTree, Section, useAction } from './shared'

export type ImportSource = 'files' | 'directory' | 'server'
export interface ImportHandle { open: (source: ImportSource) => void }

export default function ImportDictionaries({ ref, onStart }: { ref: Ref<ImportHandle>; onStart: () => void }) {
  const [source, setSource] = useState<ImportSource>('files')
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [hostPath, setHostPath] = useState('')
  const uploadRef = useRef<HTMLInputElement>(null)
  const directoryRef = useRef<HTMLInputElement>(null)
  const { pending, error, run } = useAction()
  useImperativeHandle(ref, () => ({
    open(next) {
      if (pending) return
      setSource(next)
      if (next === 'files') uploadRef.current?.click()
      else if (next === 'directory') directoryRef.current?.click()
      else { setPreview(null); onStart() }
    },
  }))
  async function upload(files: FileList | null) {
    if (!files?.length) return
    setPreview(null)
    onStart()
    const body = new FormData()
    for (const file of files) body.append('files', file, file.webkitRelativePath || file.name)
    const result = await run(
      () => api<ImportPreview>('/imports', { method: 'POST', body }),
      '文件已上传，请检查关联',
    )
    if (result) setPreview(result)
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
      {source === 'server' && (
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
                disabled={pending || !hostPath.trim()}
                onClick={async () => {
                  const result = await run(
                    () =>
                      api<ImportPreview>('/imports/path', {
                        method: 'POST',
                        body: JSON.stringify({ path: hostPath.trim() }),
                      }),
                    '目录已扫描',
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
      {pending && (
        <p role="status" className="text-sm text-muted-foreground">
          正在处理文件，请勿关闭页面…
        </p>
      )}
      {preview && (
        <div className="space-y-5">
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
                pending ||
                !preview.candidates.length ||
                preview.candidates.some((candidate) => !candidate.name.trim())
              }
              onClick={async () => {
                const result = await run(
                  () =>
                    api(`/imports/${encodeURIComponent(preview.id)}/commit`, {
                      method: 'POST',
                      body: JSON.stringify({
                        dictionaries: preview.candidates.map(({ mdx, name, resources }) => ({
                          mdx,
                          name: name.trim(),
                          resources,
                        })),
                      }),
                    }),
                  '词典已导入',
                )
                if (result !== undefined) setPreview(null)
              }}
            >
              确认导入 {preview.candidates.length} 本词典
            </Button>
            <Button variant="ghost" disabled={pending} onClick={() => setPreview(null)}>
              取消导入
            </Button>
          </div>}
        </div>
      )}
    </Section>
  )
}
