import type { HTMLAttributes } from 'react'

import { cn } from '../../lib/cn'

interface GlassCardProps extends HTMLAttributes<HTMLDivElement> {
  interactive?: boolean
}

export function GlassCard({ className, interactive = false, children, ...rest }: GlassCardProps) {
  return (
    <div
      className={cn(
        'glass-deep rounded-glass shadow-glass',
        interactive &&
          'transition-all duration-200 hover:-translate-y-0.5 hover:border-white/15 hover:bg-white/[0.07]',
        className,
      )}
      {...rest}
    >
      {children}
    </div>
  )
}
