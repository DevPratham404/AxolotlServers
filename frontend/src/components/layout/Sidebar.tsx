import type { ReactNode } from 'react'

import { cn } from '../../lib/cn'

export type View = 'dashboard' | 'servers' | 'settings'

interface SidebarProps {
  view: View
  onNavigate: (view: View) => void
  username: string
  onLogout: () => void
}

const NAV: Array<{ id: View; label: string; icon: ReactNode }> = [
  {
    id: 'dashboard',
    label: 'Dashboard',
    icon: (
      <>
        <rect x="3" y="3" width="7" height="7" rx="1.5" />
        <rect x="14" y="3" width="7" height="7" rx="1.5" />
        <rect x="3" y="14" width="7" height="7" rx="1.5" />
        <rect x="14" y="14" width="7" height="7" rx="1.5" />
      </>
    ),
  },
  {
    id: 'servers',
    label: 'Servers',
    icon: (
      <>
        <rect x="3" y="4" width="18" height="6" rx="2" />
        <rect x="3" y="14" width="18" height="6" rx="2" />
        <path d="M7 7h.01M7 17h.01" />
      </>
    ),
  },
  {
    id: 'settings',
    label: 'Settings',
    icon: (
      <>
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21a2 2 0 1 1-4 0v-.09A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H3a2 2 0 1 1 0-4h.09A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V3a2 2 0 1 1 4 0v.09A1.7 1.7 0 0 0 15.4 4.6a1.7 1.7 0 0 0 1.88-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.7 1.7 0 0 0 19.4 9c.2.3.4.7.6 1 .3.3.7.4 1.1.4H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.51.6Z" />
      </>
    ),
  },
]

export function Sidebar({ view, onNavigate, username, onLogout }: SidebarProps) {
  const initial = username.trim().charAt(0).toUpperCase() || 'A'

  return (
    <aside className="fixed inset-y-0 left-0 z-20 flex w-[76px] flex-col border-r border-white/[0.07] bg-black/35 backdrop-blur-2xl lg:w-64">
      <div className="flex h-20 items-center justify-center border-b border-white/[0.06] px-4 lg:justify-start lg:px-6">
        <span className="lg:hidden">
          <span className="grid size-10 place-items-center rounded-xl border border-white/10 bg-black/40">
            <svg viewBox="0 0 24 24" className="size-5" aria-hidden="true">
              <path d="M12 2 21 7v10l-9 5-9-5V7z" fill="none" stroke="#3cf5a6" strokeWidth="1.4" />
              <path d="m7 9.5 5 2.8 5-2.8M12 12.3V17" fill="none" stroke="#35d6f5" strokeWidth="1.2" />
            </svg>
          </span>
        </span>
        <span className="hidden lg:block">
          <span className="flex items-center gap-3">
            <span className="grid size-10 place-items-center rounded-xl border border-white/10 bg-black/40 shadow-glow-emerald">
              <svg viewBox="0 0 24 24" className="size-5" aria-hidden="true">
                <path d="M12 2 21 7v10l-9 5-9-5V7z" fill="none" stroke="#3cf5a6" strokeWidth="1.4" />
                <path d="m7 9.5 5 2.8 5-2.8M12 12.3V17" fill="none" stroke="#35d6f5" strokeWidth="1.2" />
              </svg>
            </span>
            <span className="leading-tight">
              <span className="block text-sm font-semibold tracking-tight">AxolotlServers</span>
              <span className="block font-mono text-[10px] tracking-[0.2em] text-text-dim uppercase">
                node / control
              </span>
            </span>
          </span>
        </span>
      </div>

      <nav className="flex flex-1 flex-col gap-1.5 px-3 py-6" aria-label="Primary">
        {NAV.map((item) => {
          const active = view === item.id
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => onNavigate(item.id)}
              aria-current={active ? 'page' : undefined}
              title={item.label}
              className={cn(
                'group relative flex items-center gap-3 rounded-xl px-3 py-3 text-sm transition-all duration-150',
                'justify-center lg:justify-start',
                active
                  ? 'bg-white/[0.08] text-text-primary'
                  : 'text-text-muted hover:bg-white/[0.04] hover:text-text-primary',
              )}
            >
              {active ? (
                <span className="absolute inset-y-2 left-0 w-0.5 rounded-full bg-neon-emerald shadow-glow-emerald" />
              ) : null}
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeLinejoin="round"
                className={cn('size-5 shrink-0', active && 'text-neon-emerald')}
                aria-hidden="true"
              >
                {item.icon}
              </svg>
              <span className="hidden lg:inline">{item.label}</span>
            </button>
          )
        })}
      </nav>

      <div className="border-t border-white/[0.06] p-3">
        <div className="flex items-center justify-center gap-3 rounded-xl px-2 py-2 lg:justify-start">
          <span className="grid size-9 shrink-0 place-items-center rounded-full border border-neon-emerald/25 bg-neon-emerald/10 text-sm font-semibold text-neon-emerald">
            {initial}
          </span>
          <span className="hidden min-w-0 flex-1 leading-tight lg:block">
            <span className="block truncate text-xs font-medium">{username}</span>
            <span className="block font-mono text-[10px] text-text-dim">administrator</span>
          </span>
          <button
            type="button"
            onClick={onLogout}
            title="Sign out"
            aria-label="Sign out"
            className="hidden size-8 shrink-0 place-items-center rounded-lg text-text-dim transition hover:bg-white/5 hover:text-neon-rose lg:grid"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="size-4">
              <path d="M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3M10 17l5-5-5-5M15 12H3" />
            </svg>
          </button>
        </div>
        <button
          type="button"
          onClick={onLogout}
          className="mt-2 flex w-full items-center justify-center rounded-lg py-2 text-text-dim transition hover:bg-white/5 hover:text-neon-rose lg:hidden"
          aria-label="Sign out"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="size-4">
            <path d="M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3M10 17l5-5-5-5M15 12H3" />
          </svg>
        </button>
      </div>
    </aside>
  )
}
