import { cn } from '../../lib/cn'
import { STATUS_META } from '../../lib/servers'
import type { ServerStatus } from '../../lib/types'

export function StatusPill({ status, className }: { status: ServerStatus; className?: string }) {
  const meta = STATUS_META[status]
  const animated = status === 'provisioning' || status === 'starting' || status === 'stopping'

  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-[10px] tracking-[0.14em] uppercase',
        meta.chip,
        meta.text,
        className,
      )}
    >
      <span
        className={cn('size-1.5 rounded-full', meta.dot)}
        style={animated ? { animation: 'pulse-glow 1.4s ease-in-out infinite' } : undefined}
      />
      {meta.label}
    </span>
  )
}
