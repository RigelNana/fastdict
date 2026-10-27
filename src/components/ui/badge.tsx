import type { ComponentProps } from 'react'
import { cn } from '@/lib/utils'

export function Badge({ className, variant = 'secondary', ...props }: ComponentProps<'span'> & {
  variant?: 'secondary' | 'destructive'
}) {
  return <span data-slot="badge" className={cn(
    'inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-0.5 text-xs font-medium leading-5',
    variant === 'destructive' ? 'bg-destructive/10 text-destructive' : 'bg-secondary text-secondary-foreground',
    className,
  )} {...props} />
}
