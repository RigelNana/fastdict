import { AlertCircle, Check, CircleDashed, LoaderCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import type { IndexStatus } from '@/lib/types'

const states = {
  missing: { label: '未建立索引', icon: CircleDashed },
  building: { label: '索引构建中', icon: LoaderCircle },
  ready: { label: '索引就绪', icon: Check },
  error: { label: '索引失败', icon: AlertCircle },
}

export default function IndexBadge({ status }: { status: IndexStatus }) {
  const { label, icon: Icon } = states[status.state]
  return <Badge variant={status.state === 'error' ? 'destructive' : 'secondary'} aria-label={`全文${label}`}>
    <Icon aria-hidden className={status.state === 'building' ? 'size-3 animate-spin' : 'size-3'} />
    {label}
  </Badge>
}
