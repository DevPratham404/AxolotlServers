import { cn } from '../../lib/cn'

export function Brand({ compact = false, className }: { compact?: boolean; className?: string }) {
  return (
    <div className={cn('flex items-center gap-3', className)}>
      <span className="relative grid size-10 shrink-0 place-items-center rounded-xl border border-white/10 bg-black/40 shadow-glow-emerald">
        <svg viewBox="0 0 24 24" className="size-5" aria-hidden="true">
          <path d="M12 2 21 7v10l-9 5-9-5V7z" fill="none" stroke="#3cf5a6" strokeWidth="1.4" />
          <path d="m7 9.5 5 2.8 5-2.8M12 12.3V17" fill="none" stroke="#35d6f5" strokeWidth="1.2" />
        </svg>
        <span className="absolute -inset-2 -z-10 rounded-2xl bg-neon-emerald/15 blur-xl" />
      </span>
      {!compact ? (
        <span className="leading-tight">
          <span className="block text-sm font-semibold tracking-tight">AxolotlServers</span>
          <span className="block font-mono text-[10px] tracking-[0.2em] text-text-dim uppercase">
            node / control
          </span>
        </span>
      ) : null}
    </div>
  )
}
