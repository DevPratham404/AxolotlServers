import type { ReactNode } from 'react'

import { AuroraBackdrop } from './AuroraBackdrop'
import { Sidebar, type View } from './Sidebar'

interface AppShellProps {
  view: View
  onNavigate: (view: View) => void
  username: string
  onLogout: () => void
  children: ReactNode
}

export function AppShell({ view, onNavigate, username, onLogout, children }: AppShellProps) {
  return (
    <div className="min-h-dvh">
      <AuroraBackdrop />
      <Sidebar view={view} onNavigate={onNavigate} username={username} onLogout={onLogout} />
      <main className="min-h-dvh pl-[76px] lg:pl-64">
        <div className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8 lg:px-10 lg:py-10">{children}</div>
      </main>
    </div>
  )
}
