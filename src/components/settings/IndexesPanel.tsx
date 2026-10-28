import { useState } from 'react'
import { MoreHorizontal, RefreshCw, Trash2 } from 'lucide-react'
import { api } from '@/lib/api'
import type { Bootstrap, Dictionary } from '@/lib/types'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog'
import IndexBadge from './IndexBadge'
import { Confirm, Empty, ErrorNotice, Section, bytes, useAction } from './shared'

const jobLabels = { queued: '等待中', running: '进行中', done: '已完成', error: '失败' }
const jobKinds: Record<string, string> = { fulltext: '全文索引', headwords: '词头目录', cache: '缓存', import: '导入词典' }

export default function IndexesPanel({ data }: { data: Bootstrap }) {
  const { pending, error, run } = useAction()
  const [clearing, setClearing] = useState<Dictionary | null>(null)
  const building = data.dictionaries.some((dictionary) => dictionary.index_status.state === 'building')
  const maintenance = (target: 'headwords' | 'fulltext' | 'cache', action: 'clear' | 'rebuild', dictionary_id?: string) =>
    run(() => api('/maintenance', {
      method: 'POST', body: JSON.stringify({ target, action, ...(dictionary_id ? { dictionary_id } : {}) }),
    }), action === 'rebuild' ? '任务已提交' : '已清除')

  return (
    <div className="max-w-4xl space-y-8">
      <Section title="全文索引">
        {data.dictionaries.length > 1 && (
          <Button variant="outline" disabled={pending || building} onClick={() => void maintenance('fulltext', 'rebuild')}>
            <RefreshCw className="size-4" />更新全部
          </Button>
        )}
        {!data.dictionaries.length && <Empty>暂无词典</Empty>}
        <ul className="divide-y">
          {data.dictionaries.map((dictionary) => {
            const status = dictionary.index_status
            const busy = status.state === 'building'
            return (
              <li key={dictionary.id} className="space-y-3 py-4 first:pt-0">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">
                    <span className="min-w-0 [overflow-wrap:anywhere] text-sm font-medium">{dictionary.name}</span>
                    <IndexBadge status={status} />
                  </div>
                  <div className="flex items-center gap-1">
                    <Button variant="secondary" size="sm" disabled={pending || busy}
                      aria-label={`${status.state === 'ready' ? '更新' : '建立'} ${dictionary.name} 的全文索引`}
                      onClick={() => void maintenance('fulltext', 'rebuild', dictionary.id)}>
                      {busy ? '构建中…' : status.state === 'ready' ? '更新索引' : '建立索引'}
                    </Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="icon" aria-label={`更多 ${dictionary.name} 的索引操作`}><MoreHorizontal className="size-4" /></Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem disabled={pending || busy || status.state === 'missing'}
                          className="text-destructive focus:text-destructive" onSelect={() => setClearing(dictionary)}>
                          <Trash2 />清除索引
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </div>
                {busy && (
                  <div className="space-y-2">
                    <div role="progressbar" aria-label={`${dictionary.name} 全文索引进度`}
                      aria-valuemin={0} aria-valuemax={Math.max(1, status.total)}
                      aria-valuenow={Math.min(status.indexed, Math.max(1, status.total))}
                      className="h-1 overflow-hidden rounded-full bg-muted">
                      <div className="h-full bg-primary" style={{ width: `${status.total ? Math.min(100, status.indexed / status.total * 100) : 0}%` }} />
                    </div>
                    <p className="text-xs tabular-nums text-muted-foreground">{status.indexed.toLocaleString()} / {status.total.toLocaleString()}</p>
                  </div>
                )}
                <ErrorNotice error={status.error ?? ''} />
              </li>
            )
          })}
        </ul>
      </Section>
      <Section title="词头目录与缓存">
        <div className="flex flex-wrap gap-x-5 gap-y-2 text-sm text-muted-foreground">
          <span>{data.stats.words.toLocaleString()} 个词头</span>
          <span>{data.stats.open_readers} 部已打开</span>
          <span>缓存 {bytes(data.stats.cache_bytes)}</span>
        </div>
        <div className="flex flex-wrap gap-2">
          <Confirm title="重建全部词头目录？" description="从启用的词典重新加载词头目录，期间查询可能稍慢。不扫描文章正文。"
            label="重建词头目录" disabled={pending || !data.dictionaries.length} onConfirm={() => maintenance('headwords', 'rebuild')} />
          <Confirm title="清除词头目录？" description="清除后将无法使用词头建议与拼写查询，需重建词头目录恢复。不会删除词典。"
            label="清除词头目录" disabled={pending || !data.dictionaries.length} onConfirm={() => maintenance('headwords', 'clear')} />
          <Confirm title="释放词典缓存？" description="下一次读取会重新解压，不影响词典与分组配置。"
            label="释放缓存" disabled={pending} onConfirm={() => maintenance('cache', 'clear')} />
        </div>
      </Section>
      <Accordion type="single" collapsible>
        <AccordionItem value="jobs" className="border-0">
          <AccordionTrigger className="settings-section-title hover:no-underline">任务记录</AccordionTrigger>
          <AccordionContent>
            {!data.stats.jobs.length ? <Empty>暂无任务</Empty> : (
              <ul className="divide-y">
                {data.stats.jobs.map((job) => (
                  <li key={job.id} className="space-y-2 py-3 text-sm">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span>{jobKinds[job.kind] ?? job.kind} · {data.dictionaries.find((dictionary) => dictionary.id === job.dictionary_id)?.name ?? '全部词典'}</span>
                      <Badge variant={job.state === 'error' ? 'destructive' : 'secondary'}>{jobLabels[job.state]}</Badge>
                    </div>
                    {(job.state === 'running' || job.state === 'queued') && <p className="text-xs tabular-nums text-muted-foreground">{job.progress.toLocaleString()} / {job.total.toLocaleString()}</p>}
                    <ErrorNotice error={job.error ?? ''} />
                  </li>
                ))}
              </ul>
            )}
          </AccordionContent>
        </AccordionItem>
      </Accordion>
      <ErrorNotice error={error} />
      <AlertDialog open={!!clearing} onOpenChange={(open) => { if (!open && !pending) setClearing(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>清除“{clearing?.name}”的全文索引？</AlertDialogTitle>
            <AlertDialogDescription>全文搜索将暂停，重新建立后恢复。普通词头查询不受影响。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pending}>取消</AlertDialogCancel>
            <AlertDialogAction disabled={pending} onClick={async (event) => {
              event.preventDefault()
              if (!clearing) return
              const result = await maintenance('fulltext', 'clear', clearing.id)
              if (result !== undefined) setClearing(null)
            }}>{pending ? '清除中…' : '清除'}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
