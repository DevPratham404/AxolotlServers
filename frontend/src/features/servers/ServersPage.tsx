import { useMemo, useState } from 'react'

import { ApiError, serverApi } from '../../lib/api'
import type { ServerView } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { EmptyState, ErrorBanner } from '../../components/ui/Feedback'
import { PageHeader } from '../../components/ui/PageHeader'
import { CreateServerWizard } from './CreateServerWizard'
import { ImportServerWizard } from './ImportServerWizard'
import { ServerCard, type ServerAction } from './ServerCard'
import { useServers } from './useServers'

export function ServersPage({ onOpenServer }: { onOpenServer: (id: string) => void }) {
  const { servers, loading, error, reload } = useServers()
  const [wizardOpen, setWizardOpen] = useState(false)
  const [importOpen, setImportOpen] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [action, setAction] = useState<ServerAction>(null)
  const [actionError, setActionError] = useState<string | null>(null)

  const suggestedPort = useMemo(() => nextFreePort(servers), [servers])
  const running = servers.filter((server) => server.status === 'running').length

  async function run(server: ServerView, nextAction: Exclude<ServerAction, null>) {
    setBusyId(server.id)
    setAction(nextAction)
    setActionError(null)
    try {
      if (nextAction === 'start') await serverApi.start(server.id)
      if (nextAction === 'stop') await serverApi.stop(server.id)
      if (nextAction === 'delete') await serverApi.remove(server.id)
      await reload()
    } catch (caught) {
      setActionError(
        caught instanceof ApiError
          ? `${server.name}: ${caught.message}`
          : `${server.name}: the action failed.`,
      )
    } finally {
      setBusyId(null)
      setAction(null)
    }
  }

  return (
    <>
      <PageHeader
        eyebrow="fleet"
        title="Servers"
        description="Every managed container on this node. Power actions are forwarded to the daemon over a signed channel."
        actions={
          <>
            <span className="hidden rounded-full border border-white/10 bg-white/5 px-3 py-1.5 font-mono text-[10px] tracking-[0.14em] text-text-muted uppercase sm:inline-flex">
              {running}/{servers.length} running
            </span>
            <Button
              variant="secondary"
              onClick={() => setImportOpen(true)}
              icon={
                <svg viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M8 10.5V2.5M5 5.5 8 2.5l3 3M2.5 10.5v3h11v-3" />
                </svg>
              }
            >
              Import server
            </Button>
            <Button
              variant="primary"
              onClick={() => setWizardOpen(true)}
              icon={
                <svg viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="1.8">
                  <path d="M8 3v10M3 8h10" strokeLinecap="round" />
                </svg>
              }
            >
              New server
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

      {actionError ? (
        <div className="mb-6">
          <ErrorBanner>{actionError}</ErrorBanner>
        </div>
      ) : null}

      {loading && servers.length === 0 ? (
        <SkeletonGrid />
      ) : servers.length === 0 ? (
        <EmptyState
          title="No servers yet"
          description="Provision your first container: choose a runtime, set memory and CPU limits, and Cluster Console handles the rest."
          action={
            <Button variant="primary" onClick={() => setWizardOpen(true)}>
              Create your first server
            </Button>
          }
        />
      ) : (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {servers.map((server) => (
            <ServerCard
              key={server.id}
              server={server}
              busy={busyId === server.id ? action : null}
              onOpen={(value) => onOpenServer(value.id)}
              onStart={(value) => void run(value, 'start')}
              onStop={(value) => void run(value, 'stop')}
              onDelete={(value) => void run(value, 'delete')}
            />
          ))}
        </div>
      )}

      {wizardOpen ? (
        <CreateServerWizard
          suggestedPort={suggestedPort}
          onClose={() => setWizardOpen(false)}
          onCreated={() => {
            setWizardOpen(false)
            void reload()
          }}
        />
      ) : null}

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

function nextFreePort(servers: ServerView[]): number {
  const used = new Set(servers.map((server) => server.game_port))
  for (let port = 25565; port < 25665; port += 1) {
    if (!used.has(port)) return port
  }
  return 25565
}

function SkeletonGrid() {
  return (
    <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" aria-hidden="true">
      {Array.from({ length: 3 }, (_, index) => (
        <div key={index} className="glass-deep h-64 animate-pulse rounded-glass p-5">
          <div className="flex items-center gap-3">
            <div className="size-11 rounded-xl bg-white/5" />
            <div className="flex-1 space-y-2">
              <div className="h-3 w-2/3 rounded bg-white/5" />
              <div className="h-2 w-1/3 rounded bg-white/5" />
            </div>
          </div>
          <div className="mt-6 h-16 rounded-xl bg-white/5" />
        </div>
      ))}
    </div>
  )
}
