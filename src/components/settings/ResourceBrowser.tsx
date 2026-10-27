import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, apiBlob } from '@/lib/api'
import type { Dictionary, Resource, ResourceEntries } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, ErrorNotice, bytes, errorMessage } from './shared'

type Preview = {
  url: string
  kind: 'image' | 'audio' | 'font' | 'text' | 'binary'
  text?: string
  font?: FontFace
  filename: string
}
export default function ResourceBrowser({
  dictionary,
  resource,
  onClose,
  onIcon,
}: {
  dictionary: Dictionary
  resource: Resource | null
  onClose: () => void
  onIcon: (key: string) => void
}) {
  const [prefix, setPrefix] = useState('')
  const [search, setSearch] = useState('')
  const [preview, setPreview] = useState<Preview | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const request = useRef(0)
  const base = resource
    ? `/dictionaries/${encodeURIComponent(dictionary.id)}/resources/${encodeURIComponent(resource.id)}`
    : ''
  const entries = useQuery({
    queryKey: ['resource-entries', dictionary.id, dictionary.revision, resource?.id, search],
    queryFn: ({ signal }) =>
      api<ResourceEntries>(`${base}/entries?prefix=${encodeURIComponent(search)}&limit=200`, {
        signal,
      }),
    enabled: !!resource,
  })
  useEffect(() => {
    setPrefix('')
    setSearch('')
    setPreview(null)
    setError('')
    setBusy(false)
    request.current++
    return () => {
      request.current++
    }
  }, [resource?.id])
  useEffect(
    () => () => {
      if (preview) {
        URL.revokeObjectURL(preview.url)
        if (preview.font) document.fonts.delete(preview.font)
      }
    },
    [preview],
  )
  async function show(path: string) {
    const id = ++request.current
    let objectUrl: string | undefined
    setBusy(true)
    setError('')
    setPreview(null)
    try {
      const blob = await apiBlob(`${base}/content?path=${encodeURIComponent(path)}`)
      if (id !== request.current) return
      const extension = path.split('.').at(-1)?.toLowerCase() ?? ''
      const kind: Preview['kind'] = /^(png|jpe?g|gif|webp|avif|bmp|ico)$/.test(extension)
        ? 'image'
        : /^(mp3|wav|ogg|m4a|flac|aac)$/.test(extension)
          ? 'audio'
          : /^(woff2?|ttf|otf)$/.test(extension)
            ? 'font'
            : /^(css|js|html?|svg|txt|xml|json|md|csv)$/.test(extension)
              ? 'text'
              : 'binary'
      const mime =
        kind === 'font'
          ? `font/${extension}`
          : kind === 'image'
            ? `image/${extension === 'jpg' ? 'jpeg' : extension === 'ico' ? 'x-icon' : extension}`
            : kind === 'audio'
              ? `audio/${extension === 'mp3' ? 'mpeg' : extension === 'm4a' ? 'mp4' : extension}`
              : 'application/octet-stream'
      const url = URL.createObjectURL(new Blob([blob], { type: mime }))
      objectUrl = url
      const result: Preview = { url, kind, filename: path }
      if (kind === 'text')
        result.text =
          (await blob.slice(0, 256 * 1024).text()) +
          (blob.size > 256 * 1024 ? '\n\n[仅显示前 256 KB]' : '')
      if (kind === 'font') {
        result.font = await new FontFace('fastdict-resource-preview', `url(${url})`).load()
        if (id === request.current) document.fonts.add(result.font)
      }
      if (id === request.current) setPreview(result)
      else {
        URL.revokeObjectURL(url)
        if (result.font) document.fonts.delete(result.font)
      }
    } catch (cause) {
      if (objectUrl) URL.revokeObjectURL(objectUrl)
      if (id === request.current) setError(errorMessage(cause))
    } finally {
      if (id === request.current) setBusy(false)
    }
  }
  return (
    <Dialog
      open={!!resource}
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent className="max-h-[90dvh] overflow-hidden! sm:max-w-3xl">
        <ScrollArea className="max-h-[calc(90dvh-3rem)]" viewportClassName="pr-3"><div className="space-y-4">
        <DialogHeader>
          <DialogTitle className="break-all">资源浏览 · {resource?.path}</DialogTitle>
          <DialogDescription className="sr-only">文件内容预览</DialogDescription>
        </DialogHeader>
        {resource?.kind === 'mdd' && (
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault()
              setSearch(prefix)
            }}
          >
            <Input
              aria-label="MDD 资源路径前缀"
              placeholder="输入资源路径前缀"
              value={prefix}
              onChange={(event) => setPrefix(event.target.value)}
            />
            <Button type="submit" variant="outline">
              查找
            </Button>
          </form>
        )}
        {entries.isPending && <p role="status">正在读取资源目录…</p>}
        <ErrorNotice error={entries.error ? errorMessage(entries.error) : error} />
        {entries.data && (
          <>
            <ScrollArea className="max-h-60" viewportClassName="pr-3">
              {entries.data.items.length === 0 ? (
                <Empty>没有匹配的资源路径。试试更短的前缀。</Empty>
              ) : (
                <ul>
                  {entries.data.items.map((item) => (
                    <li
                      key={item.path}
                      className="flex flex-wrap items-center justify-between gap-2 border-b py-2"
                    >
                      <div className="min-w-0 flex-1">
                        <p className="break-all text-sm">{item.path}</p>
                        <span className="text-xs text-muted-foreground">{bytes(item.size)}</span>
                      </div>
                      <Button variant="ghost" disabled={busy} onClick={() => void show(item.path)}>
                        预览
                      </Button>
                      {/\.(png|jpe?g|gif|webp|avif|bmp|ico)$/i.test(item.path) && (
                        <Button
                          variant="outline"
                          onClick={() =>
                            onIcon(
                              resource?.kind === 'mdd'
                                ? [resource.key, item.path]
                                    .map((part) =>
                                      part.replace(/\\/g, '/').replace(/^\/+|\/+$/g, ''),
                                    )
                                    .filter(Boolean)
                                    .join('/')
                                : resource?.key || item.path,
                            )
                          }
                        >
                          用作图标
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </ScrollArea>
            {entries.data.truncated && (
              <p className="text-sm text-muted-foreground">
                仅显示前 200 项。请输入更精确的路径前缀继续查找。
              </p>
            )}
          </>
        )}
        {busy && (
          <p role="status" className="text-sm">
            正在载入预览…
          </p>
        )}
        {preview && (
          <div className="space-y-3 border-t pt-4">
            <p className="break-all text-sm font-medium">{preview.filename}</p>
            {preview.kind === 'image' && (
              <img
                src={preview.url}
                alt={preview.filename}
                className="max-h-72 max-w-full object-contain"
              />
            )}
            {preview.kind === 'audio' && <audio controls src={preview.url} className="w-full" />}
            {preview.kind === 'text' && (
              <ScrollArea className="max-h-72"><pre className="whitespace-pre-wrap break-all text-xs leading-6">
                {preview.text}
              </pre></ScrollArea>
            )}
            {preview.kind === 'font' && (
              <p
                style={{ fontFamily: 'fastdict-resource-preview' }}
                className="break-words text-3xl leading-relaxed"
              >
                词典 · 阅读 · 语言
                <br />
                The quick brown fox jumps over the lazy dog.
                <br />
                Aa Bb Cc 0123456789
              </p>
            )}
            {preview.kind === 'binary' && (
              <p className="text-sm text-muted-foreground">此二进制格式没有安全的内嵌预览。</p>
            )}
            <Button asChild variant="outline">
              <a href={preview.url} download={preview.filename.split(/[\\/]/).at(-1)}>
                下载此资源
              </a>
            </Button>
          </div>
        )}
        </div></ScrollArea>
      </DialogContent>
    </Dialog>
  )
}
