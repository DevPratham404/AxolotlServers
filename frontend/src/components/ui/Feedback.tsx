import type { ReactNode } from 'react'

import { cn } from '../../lib/cn'

export function ErrorBanner({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      role="alert"
      className={cn(
        'flex items-start gap-2.5 rounded-xl border border-neon-rose/25 bg-neon-rose/10 px-3.5 py-3 text-xs leading-relaxed text-neon-rose',
        className,
      )}
    >
      <svg viewBox="0 0 20 20" className="mt-0.5 size-4 shrink-0" fill="currentColor" aria-hidden="true">
        <path
          fillRule="evenodd"
          d="M10 18a8 8 0 1 0 0-16 8 8 0 0 0 0 16Zm-.75-11.5a.75.75 0 0 1 1.5 0v4a.75.75 0 0 1-1.5 0v-4ZM10 14.5a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z"
          clipRule="evenodd"
        />
      </svg>
      <span>{children}</span>
    </div>
  )
}

export function EmptyState({
  title,
  description,
  action,
  icon,
}: {
  title: string
  description: string
  action?: ReactNode
  icon?: ReactNode
}) {
  return (
    <div className="glass-deep flex flex-col items-center rounded-glass px-6 py-16 text-center">
      <div className="mb-5 grid size-14 place-items-center rounded-2xl border border-white/10 bg-white/5 text-neon-emerald">
        {icon ?? (
          <svg viewBox="0 0 24 24" className="size-6" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 3 20 7.5v9L12 21l-8-4.5v-9z" />
            <path d="M4 7.5 12 12l8-4.5M12 12v9" />
          </svg>
        )}
      </div>
      <h3 className="text-base font-semibold">{title}</h3>
      <p className="mt-2 max-w-sm text-sm leading-relaxed text-text-muted">{description}</p>
      {action ? <div className="mt-6">{action}</div> : null}
    </div>
  )
}
