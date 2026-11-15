import { useState } from 'react'
import type { ReactNode } from 'react'

import { cn } from '../../lib/cn'
import { formatCpu, formatRam, serverTypeMeta } from '../../lib/servers'
import type { ServerView } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { EmptyState, ErrorBanner } from '../../components/ui/Feedback'
import { GlassCard } from '../../components/ui/GlassCard'
import { PageHeader } from '../../components/ui/PageHeader'
import { StatusPill } from '../../components/ui/StatusPill'
import { useAuth } from '../../auth/auth-context'
import { ImportServerWizard } from '../servers/ImportServerWizard'
import { useServers } from '../servers/useServers'

export function DashboardPage({ onOpenServers }: { onOpenServers: () => void }) {
  const { user } = useAuth()
  const { servers, loading, error, reload } = useServers()
  const [importOpen, setImportOpen] = useState(false)

  const running = servers.filter((server) => server.status === 'running')
  const allocatedRam = servers.reduce((total, server) => total + server.ram_mb, 0)
  const allocatedCpu = servers.reduce((total, server) => total + server.cpu_limit, 0)
  const recent = servers.slice(0, 4)

  return (
    <>
      <PageHeader
        eyebrow="overview"
        title={greeting(user?.username ?? 'operator')}
        description="A live read of this node. Every value below comes from the control-plane database; container state follows the daemon."
        actions={
          <>
            <Button
              variant="secondary"
              onClick={() => setImportOpen(true)}
              icon={
                <svg viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M8 10.5V2.5M5 5.5 8 2.5l3 3M2.5 10.5v3h11v-3" />
                </svg>
              }
            >
              Import Server
            </Button>
            <Button
              variant="primary"
              onClick={onOpenServers}
              icon={
                <svg viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="1.8">
                  <path d="M8 3v10M3 8h10" strokeLinecap="round" />
                </svg>
              }
            >
              New Server
            </Button>
            <Button variant="secondary" onClick={() => void reload()} loading={loading}>
              Refresh
            </Button>
          </>
        }
      />

      {error ? (
        <div className="mb-6">
          <ErrorBanner>
            {error}{' '}
            <button type="button" onClick={() => void reload()} className="underline">
              Retry
            </button>
          </ErrorBanner>
        </div>
      ) : null}

      <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard
          label="Managed servers"
          value={String(servers.length)}
          detail={servers.length === 0 ? 'none provisioned' : `${running.length} running`}
          accent="text-text-primary"
          icon={
            <path d="M3 5h18v6H3zM3 13h18v6H3zM7 8h.01M7 16h.01" />
          }
        />
        <StatCard
          label="Running now"
          value={String(running.length)}
          detail={servers.length === 0 ? '—' : `${servers.length - running.length} offline`}
          accent="text-neon-emerald"
          icon={<path d="M4 3.5v17l15-8.5z" />}
        />
        <StatCard
          label="Memory reserved"
          value={formatRam(allocatedRam)}
          detail="hard limits, no swap"
          accent="text-neon-cyan"
          icon={<path d="M4 7h16v10H4zM8 7V4M16 7V4M8 17v3M16 17v3" />}
        />
        <StatCard
          label="CPU reserved"
          value={formatCpu(allocatedCpu)}
          detail="nano-cpus"
          accent="text-neon-violet"
          icon={<path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3M6 6h12v12H6z" />}
        />
      </section>

      <section className="mt-8 grid gap-4 lg:grid-cols-[1.45fr_1fr]">
        <GlassCard className="animate-rise p-5 sm:p-6">
          <div className="mb-4 flex items-end justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold">Recent servers</h2>
              <p className="mt-1 text-xs text-text-muted">Newest first, from the control plane.</p>
            </div>
            <button
              type="button"
              onClick={onOpenServers}
              className="text-[11px] font-medium text-neon-emerald transition hover:text-[#7cffd0]"
            >
              Open servers →
            </button>
          </div>

          {loading && servers.length === 0 ? (
            <div className="space-y-2" aria-hidden="true">
              {Array.from({ length: 3 }, (_, index) => (
                <div key={index} className="h-16 animate-pulse rounded-xl bg-white/[0.04]" />
              ))}
            </div>
          ) : recent.length === 0 ? (
            <EmptyState
              title="The node is empty"
              description="Spin up a Paper or Vanilla server and it will appear here with its live status."
              action={
                <Button variant="primary" onClick={onOpenServers}>
                  Create a server
                </Button>
              }
            />
          ) : (
            <ul className="space-y-2">
              {recent.map((server) => (
                <RecentRow key={server.id} server={server} onOpen={onOpenServers} />
              ))}
            </ul>
          )}
        </GlassCard>

        <GlassCard className="animate-rise p-5 sm:p-6">
          <h2 className="text-sm font-semibold">Node posture</h2>
          <p className="mt-1 text-xs text-text-muted">
            Facts the control plane can prove without asking Docker.
          </p>

          <ul className="mt-5 space-y-3 text-xs">
            <PostureRow label="Control plane" value="Rust · axum · SQLite" tone="ok" />
            <PostureRow label="Daemon auth" value="HMAC-SHA256 · 300 s window" tone="ok" />
            <PostureRow
              label="Docker socket"
              value="daemon only"
              tone="ok"
            />
            <PostureRow label="Session" value="Bearer JWT · localStorage" tone="neutral" />
          </ul>

          <div className="mt-5 rounded-xl border border-neon-emerald/20 bg-neon-emerald/[0.05] px-3.5 py-3 text-[11px] leading-relaxed text-neon-emerald/90">
            Limits are enforced by the container runtime, not by the JVM. A server cannot exceed its
            memory, CPU or PID budget.
          </div>
        </GlassCard>
      </section>

      {importOpen ? (
        <ImportServerWizard
          onClose={() => setImportOpen(false)}
          onImported={() => {
            setImportOpen(false)
            void reload()
          }}
        />
      ) : null}
    </>
  )
}

function greeting(username: string): string {
  const hour = new Date().getHours()
  const part = hour < 5 ? 'Working late' : hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening'
  return `${part}, ${username}`
}

function StatCard({
  label,
  value,
  detail,
  accent,
  icon,
}: {
  label: string
  value: string
  detail: string
  accent: string
  icon: ReactNode
}) {
  return (
    <GlassCard className="animate-rise p-5">
      <div className="flex items-start justify-between">
        <span className="text-[10px] tracking-[0.18em] text-text-dim uppercase">{label}</span>
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={cn('size-4 opacity-70', accent)}
          aria-hidden="true"
        >
          {icon}
        </svg>
      </div>
      <p className={cn('mt-4 font-mono text-2xl font-medium tracking-tight', accent)}>{value}</p>
      <p className="mt-1 text-[11px] text-text-dim">{detail}</p>
    </GlassCard>
  )
}

function RecentRow({ server, onOpen }: { server: ServerView; onOpen: () => void }) {
  const meta = serverTypeMeta(server.server_type)
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        className="flex w-full items-center gap-3 rounded-xl border border-transparent px-3 py-3 text-left transition hover:border-white/[0.08] hover:bg-white/[0.04]"
      >
        <span
          className={cn(
            'grid size-9 shrink-0 place-items-center rounded-lg border border-white/10 bg-black/30 font-mono text-sm font-semibold',
            meta.accent,
          )}
        >
          {meta.monogram}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">{server.name}</span>
          <span className="mt-0.5 block font-mono text-[10px] text-text-dim">
            {meta.label} · {formatRam(server.ram_mb)} · {formatCpu(server.cpu_limit)}
          </span>
        </span>
        <StatusPill status={server.status} />
      </button>
    </li>
  )
}

function PostureRow({
  label,
  value,
  tone,
}: {
  label: string
  value: string
  tone: 'ok' | 'neutral'
}) {
  return (
    <li className="flex items-center justify-between gap-4 border-b border-white/[0.06] pb-3 last:border-0 last:pb-0">
      <span className="text-text-dim">{label}</span>
      <span
        className={cn(
          'text-right font-mono text-[10px]',
          tone === 'ok' ? 'text-neon-emerald' : 'text-text-muted',
        )}
      >
        {value}
      </span>
    </li>
  )
}
