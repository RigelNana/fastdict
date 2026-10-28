import type { ComponentProps } from 'react'
import { ScrollArea as Primitive } from 'radix-ui'
import { cn } from '@/lib/utils'

export function ScrollArea({ className, viewportClassName, children, ...props }: ComponentProps<typeof Primitive.Root> & { viewportClassName?: string }) {
  return (
    <Primitive.Root type="scroll" scrollHideDelay={1200} className={cn('scroll-area relative min-w-0', className)} {...props}>
      <Primitive.Viewport className={cn('scroll-area-viewport w-full rounded-[inherit] outline-none', viewportClassName)}>
        {children}
      </Primitive.Viewport>
      <Primitive.Scrollbar orientation="vertical" className="scroll-area-track">
        <Primitive.Thumb className="scroll-area-thumb" />
      </Primitive.Scrollbar>
    </Primitive.Root>
  )
}
