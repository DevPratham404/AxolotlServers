import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

import { ApiError, serverApi } from '../../lib/api'
import type { AddonProjectType, InstalledAddon, ServerType } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { EmptyState, ErrorBanner } from '../../components/ui/Feedback'
import { GlassCard } from '../../components/ui/GlassCard'
import { Spinner } from '../../components/ui/Spinner'

const MODRINTH_API = 'https://api.modrinth.com/v2'
const SEARCH_DEBOUNCE_MS = 350
const PROJECT_STORAGE_PREFIX = 'cluster.addons.projects.'

const PROJECT_TYPES: Array<{ value: AddonProjectType; label: string; hint: string }> = [
  { value: 'mod', label: 'Mods', hint: 'fabric / forge' },
  { value: 'plugin', label: 'Plugins', hint: 'paper / velocity / bungee' },
  { value: 'datapack', label: 'Datapacks', hint: 'world/datapacks' },
  { value: 'resourcepack', label: 'Resource Packs', hint: 'resourcepacks' },
  { value: 'modpack', label: 'Modpacks', hint: 'fabric / forge' },
]

interface ModrinthHit {
  project_id: string
  slug: string
  title: string
  description: string
  author: string
  icon_url: string | null
  downloads: number
  categories: string[]
}

interface ModrinthProject {
  id: string
  slug: string
  title: string
  description: string
  body: string | null
  icon_url: string | null
  downloads: number
  categories: string[]
}

interface ModrinthVersionFile {
  url: string
  filename: string
  primary: boolean
  size: number
}

interface ModrinthVersion {
  id: string
  version_number: string
  name: string
  version_type: 'release' | 'beta' | 'alpha'
  loaders: string[]
  game_versions: string[]
  date_published: string
  files: ModrinthVersionFile[]
}

interface AppStorePanelProps {
  serverId: string
  serverType: ServerType
  gameVersion: string
}

type StoreView = 'discover' | 'installed'

/**
 * Modrinth browser. Search, project details and version metadata come directly
 * from Modrinth in the browser; only the chosen version file URL is sent to our
 * API, which resolves the destination folder and streams the download.
 */
export function AppStorePanel({ serverId, serverType, gameVersion }: AppStorePanelProps) {
  const [view, setView] = useState<StoreView>('discover')
  const [projectType, setProjectType] = useState<AddonProjectType>(() =>
    defaultProjectType(serverType),
  )
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<ModrinthHit[]>([])
  const [searching, setSearching] = useState(false)
  const [searchError, setSearchError] = useState<string | null>(null)
  const [installed, setInstalled] = useState<InstalledAddon[]>([])
  const [loadingInstalled, setLoadingInstalled] = useState(false)
  const [installedError, setInstalledError] = useState<string | null>(null)
  const [projectFiles, setProjectFiles] = useState<Record<string, string>>(() =>
    readProjectFiles(serverId),
  )
  const [installing, setInstalling] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<string | null>(null)
  const [detailHit, setDetailHit] = useState<ModrinthHit | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const searchId = useRef(0)

  const availableTypes = PROJECT_TYPES.filter((option) =>
    supportsProjectType(option.value, serverType),
  )

  const refreshInstalled = useCallback(async () => {
    setLoadingInstalled(true)
    setInstalledError(null)
    try {
      setInstalled(await serverApi.listAddons(serverId))
    } catch (caught) {
      setInstalledError(
        caught instanceof ApiError ? caught.message : 'Could not list installed add-ons.',
      )
    } finally {
      setLoadingInstalled(false)
    }
  }, [serverId])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    void refreshInstalled()
  }, [refreshInstalled])

  const runSearch = useCallback(async () => {
    const id = ++searchId.current
    setSearching(true)
    setSearchError(null)
    try {
      const hits = await searchModrinth(query, projectType, serverType)
      if (id === searchId.current) setResults(hits)
    } catch (caught) {
      if (id !== searchId.current) return
      setSearchError(
        caught instanceof Error ? caught.message : 'Could not reach the Modrinth API.',
      )
    } finally {
      if (id === searchId.current) setSearching(false)
    }
  }, [projectType, query, serverType])

  useEffect(() => {
    const timer = window.setTimeout(() => void runSearch(), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [runSearch])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(null), 5000)
    return () => window.clearTimeout(timer)
  }, [notice])

  const groupedInstalled = useMemo(() => {
    const groups = new Map<string, InstalledAddon[]>()
    for (const addon of installed) {
      const existing = groups.get(addon.directory)
      if (existing) existing.push(addon)
      else groups.set(addon.directory, [addon])
    }
    return [...groups.entries()].sort(([left], [right]) => left.localeCompare(right))
  }, [installed])

  function rememberProject(projectId: string, filename: string) {
    setProjectFiles((current) => {
      const next = { ...current, [projectId]: filename }
      saveProjectFiles(serverId, next)
      return next
    })
  }

  function forgetProjectFilename(filename: string) {
    setProjectFiles((current) => {
      const next = Object.fromEntries(
        Object.entries(current).filter(([, value]) => value !== filename),
      )
      saveProjectFiles(serverId, next)
      return next
    })
  }

  async function installLatest(hit: ModrinthHit) {
    setInstalling(hit.project_id)
    setActionError(null)
    setNotice(null)
    try {
      const version = await latestVersion(
        hit.project_id,
        projectType,
        serverType,
        gameVersion,
      )
      const addon = await serverApi.installAddon(serverId, version.url, projectType)
      rememberProject(hit.project_id, addon.name)
      setNotice(`Installed ${addon.name}. Restart the server to load it.`)
      await refreshInstalled()
    } catch (caught) {
      setActionError(
        caught instanceof ApiError || caught instanceof Error
          ? caught.message
          : `Could not install ${hit.title}.`,
      )
    } finally {
      setInstalling(null)
    }
  }

  async function remove(addon: InstalledAddon) {
    if (!window.confirm(`Delete ${addon.name}? This removes the file from the server.`)) return
    setDeleting(addon.path)
    setActionError(null)
    setNotice(null)
    try {
      await serverApi.deleteFile(serverId, addon.path)
      forgetProjectFilename(addon.name)
      setNotice(`Deleted ${addon.name}.`)
      await refreshInstalled()
    } catch (caught) {
      setActionError(
        caught instanceof ApiError ? caught.message : `Could not delete ${addon.name}.`,
      )
    } finally {
      setDeleting(null)
    }
  }

  const detailInstalled = detailHit
    ? findInstalledForHit(detailHit, installed, projectType, projectFiles)
    : null

  return (
    <div className="space-y-4">
      <GlassCard className="animate-rise overflow-hidden">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.07] px-5 py-4">
          <div className="flex items-center gap-3">
            <span className="grid size-8 place-items-center rounded-lg border border-white/10 bg-black/25 text-neon-emerald">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="size-4">
                <path d="M4 7.5 12 3l8 4.5v9L12 21l-8-4.5z" strokeLinejoin="round" />
                <path d="M4 7.5 12 12l8-4.5M12 12v9" strokeLinejoin="round" />
              </svg>
            </span>
            <div>
              <h2 className="text-sm font-semibold">App Store</h2>
              <p className="mt-0.5 font-mono text-[10px] text-text-dim">
                Modrinth · installs land in the matching server folder
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <div className="flex items-center gap-0.5 rounded-lg border border-white/10 bg-black/25 p-0.5">
              <ViewTab active={view === 'discover'} onClick={() => setView('discover')}>
                Discover
              </ViewTab>
              <ViewTab active={view === 'installed'} onClick={() => setView('installed')}>
                Installed
                <span className="ml-1 rounded bg-white/10 px-1 font-mono text-[8px]">
                  {installed.length}
                </span>
              </ViewTab>
            </div>
            {view === 'discover' ? (
              <label className="relative">
                <span className="sr-only">Filter by project type</span>
                <select
                  value={projectType}
                  onChange={(event) => {
                    setProjectType(event.target.value as AddonProjectType)
                    setResults([])
                    setSearchError(null)
                  }}
                  className="h-9 appearance-none rounded-lg border border-white/10 bg-black/30 pr-8 pl-3 font-mono text-[10px] tracking-[0.08em] text-text-primary uppercase outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15"
                >
                  {availableTypes.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label} · {option.hint}
                    </option>
                  ))}
                </select>
                <span className="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-[9px] text-text-dim">
                  ▼
                </span>
              </label>
            ) : null}
            <span className="rounded-full border border-white/10 bg-white/5 px-2.5 py-1 font-mono text-[9px] tracking-[0.1em] text-text-dim uppercase">
              {serverType} · {gameVersion}
            </span>
          </div>
        </header>

        {notice ? (
          <p className="border-b border-neon-emerald/15 bg-neon-emerald/[0.06] px-5 py-2.5 text-[11px] text-neon-emerald">
            {notice}
          </p>
        ) : null}
        {actionError ? (
          <p
            role="alert"
            className="border-b border-neon-rose/20 bg-neon-rose/[0.07] px-5 py-2.5 text-[11px] text-neon-rose"
          >
            {actionError}
          </p>
        ) : null}

        {installing && projectType === 'modpack' ? (
          <div
            role="status"
            aria-live="polite"
            className="flex items-center gap-3 border-b border-neon-amber/20 bg-neon-amber/[0.07] px-5 py-3"
          >
            <Spinner className="size-4 text-neon-amber" />
            <div>
              <p className="text-xs font-medium text-neon-amber">
                Installing Modpack (This will take a few minutes)…
              </p>
              <p className="mt-0.5 font-mono text-[10px] text-text-dim">
                Downloading pack files concurrently and applying overrides. Keep this tab open.
              </p>
            </div>
          </div>
        ) : null}

        {view === 'discover' ? (
          <div className="p-5">
            <label className="relative block">
              <span className="sr-only">Search Modrinth</span>
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-text-dim"
                aria-hidden="true"
              >
                <circle cx="11" cy="11" r="6.5" />
                <path d="m16 16 4 4" strokeLinecap="round" />
              </svg>
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={`Search Modrinth for ${projectTypeLabel(projectType)}…`}
                className="h-11 w-full rounded-xl border border-white/10 bg-black/30 pr-24 pl-9 text-sm text-text-primary outline-none transition placeholder:text-text-dim focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15"
              />
              <span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 font-mono text-[9px] tracking-[0.1em] text-text-dim uppercase">
                {searching ? 'searching…' : `${results.length} results`}
              </span>
            </label>

            {searchError ? (
              <div className="mt-4">
                <ErrorBanner>{searchError}</ErrorBanner>
              </div>
            ) : null}

            {!searchError && results.length === 0 && !searching ? (
              <div className="mt-4">
                <EmptyState
                  title="No projects found"
                  description={`Modrinth has nothing matching “${query || projectType}” for ${serverType}. Try another search term.`}
                />
              </div>
            ) : (
              <ul className="mt-4 grid gap-3 md:grid-cols-2 2xl:grid-cols-3">
                {results.map((hit) => (
                  <li key={hit.project_id}>
                    <ProjectCard
                      hit={hit}
                      installedAddon={findInstalledForHit(
                        hit,
                        installed,
                        projectType,
                        projectFiles,
                      )}
                      installing={installing === hit.project_id}
                      deleting={deleting}
                      onOpen={() => setDetailHit(hit)}
                      onInstall={() => void installLatest(hit)}
                      onDelete={(addon) => void remove(addon)}
                    />
                  </li>
                ))}
              </ul>
            )}
          </div>
        ) : (
          <div className="p-5">
            {installedError ? (
              <ErrorBanner>{installedError}</ErrorBanner>
            ) : loadingInstalled && installed.length === 0 ? (
              <p className="py-10 text-center font-mono text-[10px] tracking-[0.12em] text-text-dim uppercase">
                loading installed add-ons…
              </p>
            ) : installed.length === 0 ? (
              <EmptyState
                title="Nothing installed yet"
                description="Install a project from Discover and it will be grouped here by its server folder."
              />
            ) : (
              <div className="space-y-5">
                <div className="flex items-center justify-between">
                  <p className="text-[11px] text-text-dim">
                    {installed.length} files across {groupedInstalled.length} folders
                  </p>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void refreshInstalled()}
                    loading={loadingInstalled}
                  >
                    Refresh
                  </Button>
                </div>

                {groupedInstalled.map(([directory, addons]) => (
                  <section
                    key={directory}
                    className="overflow-hidden rounded-xl border border-white/[0.07] bg-black/20"
                  >
                    <header className="flex items-center justify-between gap-3 border-b border-white/[0.06] bg-white/[0.02] px-4 py-2.5">
                      <span className="font-mono text-[10px] tracking-[0.1em] text-text-muted uppercase">
                        {directory}/
                      </span>
                      <span className="font-mono text-[9px] text-text-dim">
                        {addons.length} {addons.length === 1 ? 'file' : 'files'}
                      </span>
                    </header>
                    <ul className="divide-y divide-white/[0.05]">
                      {addons.map((addon) => (
                        <InstalledRow
                          key={addon.path}
                          addon={addon}
                          deleting={deleting === addon.path}
                          disabled={deleting !== null}
                          onDelete={() => void remove(addon)}
                        />
                      ))}
                    </ul>
                  </section>
                ))}
              </div>
            )}
          </div>
        )}
      </GlassCard>

      {detailHit ? (
        <ProjectDetailsModal
          hit={detailHit}
          serverId={serverId}
          serverType={serverType}
          gameVersion={gameVersion}
          projectType={projectType}
          installedAddon={detailInstalled}
          onClose={() => setDetailHit(null)}
          onInstalled={(addon) => {
            rememberProject(detailHit.project_id, addon.name)
            setNotice(`Installed ${addon.name}. Restart the server to load it.`)
            void refreshInstalled()
          }}
          onDelete={(addon) => {
            // Close first so delete progress and errors surface on the page behind the modal.
            setDetailHit(null)
            void remove(addon)
          }}
        />
      ) : null}
    </div>
  )
}

function ViewTab({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={
        active
          ? 'rounded-md bg-white/10 px-2.5 py-1.5 font-mono text-[9px] tracking-[0.1em] text-text-primary uppercase'
          : 'rounded-md px-2.5 py-1.5 font-mono text-[9px] tracking-[0.1em] text-text-dim uppercase transition hover:text-text-muted'
      }
    >
      {children}
    </button>
  )
}

function ProjectCard({
  hit,
  installedAddon,
  installing,
  deleting,
  onOpen,
  onInstall,
  onDelete,
}: {
  hit: ModrinthHit
  installedAddon: InstalledAddon | null
  installing: boolean
  deleting: string | null
  onOpen: () => void
  onInstall: () => void
  onDelete: (addon: InstalledAddon) => void
}) {
  return (
    <article
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onOpen()
        }
      }}
      className="flex h-full cursor-pointer flex-col rounded-xl border border-white/[0.07] bg-black/20 p-3.5 transition hover:border-white/15 hover:bg-white/[0.03] focus-visible:ring-2 focus-visible:ring-neon-emerald/40 focus-visible:outline-none"
    >
      <div className="flex items-start gap-3">
        <AddonIcon title={hit.title} iconUrl={hit.icon_url} />
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-sm font-semibold text-text-primary" title={hit.title}>
            {hit.title}
          </h3>
          <p className="mt-0.5 truncate font-mono text-[10px] text-text-dim">
            by {hit.author || 'unknown'} · {formatDownloads(hit.downloads)} downloads
          </p>
        </div>
        {installedAddon ? (
          <span className="rounded border border-neon-emerald/30 bg-neon-emerald/10 px-1 py-px font-mono text-[8px] tracking-[0.1em] text-neon-emerald uppercase">
            installed
          </span>
        ) : null}
      </div>

      <p className="mt-2 line-clamp-2 min-h-8 text-[11px] leading-relaxed text-text-muted">
        {hit.description}
      </p>

      <div className="mt-3 flex items-center gap-1.5">
        {hit.categories.slice(0, 2).map((category) => (
          <span
            key={category}
            className="rounded border border-white/10 bg-white/5 px-1.5 py-px font-mono text-[8px] tracking-[0.08em] text-text-dim uppercase"
          >
            {category}
          </span>
        ))}
        {installedAddon ? (
          <Button
            variant="danger"
            size="sm"
            className="ml-auto"
            loading={deleting === installedAddon.path}
            disabled={deleting !== null}
            onClick={(event) => {
              event.stopPropagation()
              onDelete(installedAddon)
            }}
          >
            Delete
          </Button>
        ) : (
          <Button
            variant="primary"
            size="sm"
            className="ml-auto"
            loading={installing}
            disabled={installing}
            onClick={(event) => {
              event.stopPropagation()
              onInstall()
            }}
          >
            Install
          </Button>
        )}
      </div>
    </article>
  )
}

function InstalledRow({
  addon,
  deleting,
  disabled,
  onDelete,
}: {
  addon: InstalledAddon
  deleting: boolean
  disabled: boolean
  onDelete: () => void
}) {
  return (
    <li className="flex flex-wrap items-center gap-3 px-4 py-3 transition hover:bg-white/[0.025]">
      <span className="grid size-9 shrink-0 place-items-center rounded-lg border border-white/10 bg-black/25 font-mono text-[10px] text-neon-cyan uppercase">
        {addon.name.split('.').pop()?.toUpperCase() ?? 'FILE'}
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-xs font-medium text-text-primary">{addon.name}</p>
        <p className="mt-0.5 truncate font-mono text-[10px] text-text-dim">
          {formatBytes(addon.size)}
        </p>
      </div>
      <span className="rounded border border-neon-cyan/25 bg-neon-cyan/10 px-1.5 py-px font-mono text-[8px] tracking-[0.1em] text-neon-cyan uppercase">
        {addon.project_type}
      </span>
      <Button
        variant="danger"
        size="sm"
        loading={deleting}
        disabled={disabled}
        onClick={onDelete}
      >
        Delete
      </Button>
    </li>
  )
}

function ProjectDetailsModal({
  hit,
  serverId,
  serverType,
  gameVersion,
  projectType,
  installedAddon,
  onClose,
  onInstalled,
  onDelete,
}: {
  hit: ModrinthHit
  serverId: string
  serverType: ServerType
  gameVersion: string
  projectType: AddonProjectType
  installedAddon: InstalledAddon | null
  onClose: () => void
  onInstalled: (addon: InstalledAddon) => void
  onDelete: (addon: InstalledAddon) => void
}) {
  const [project, setProject] = useState<ModrinthProject | null>(null)
  const [versions, setVersions] = useState<ModrinthVersion[]>([])
  const [selectedLoader, setSelectedLoader] = useState('')
  const [selectedGameVersion, setSelectedGameVersion] = useState('')
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [installing, setInstalling] = useState(false)
  const [installError, setInstallError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    // oxlint-disable-next-line react/set-state-in-effect
    setLoading(true)
    setLoadError(null)
    setProject(null)
    setVersions([])
    setSelectedLoader('')
    setSelectedGameVersion('')

    Promise.all([
      modrinthJson<ModrinthProject>(`/project/${encodeURIComponent(hit.project_id)}`),
      modrinthJson<ModrinthVersion[]>(
        `/project/${encodeURIComponent(hit.project_id)}/version`,
      ),
    ])
      .then(([details, allVersions]) => {
        if (cancelled) return
        setProject(details)
        setVersions(allVersions)
        const loaders = loaderOptionsFor(allVersions, projectType, serverType)
        const loader = defaultLoaderOption(loaders, projectType, serverType)
        const games = gameVersionOptionsFor(allVersions, loader, projectType)
        setSelectedLoader(loader)
        setSelectedGameVersion(defaultGameVersionOption(games, gameVersion))
      })
      .catch((caught) => {
        if (cancelled) return
        setLoadError(
          caught instanceof Error ? caught.message : 'Could not load project details.',
        )
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [gameVersion, hit.project_id, projectType, serverType])

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  const loaderOptions = loaderOptionsFor(versions, projectType, serverType)
  const gameVersionOptions = gameVersionOptionsFor(
    versions,
    selectedLoader,
    projectType,
  )
  const selectedVersion = resolveSelectedVersion(
    versions,
    selectedLoader,
    selectedGameVersion,
    projectType,
  )
  const selectedFile = selectedVersion
    ? fileForVersion(selectedVersion, projectType)
    : undefined

  function chooseLoader(loader: string) {
    setSelectedLoader(loader)
    const games = gameVersionOptionsFor(versions, loader, projectType)
    setSelectedGameVersion(defaultGameVersionOption(games, gameVersion))
  }

  async function installSelected() {
    if (!selectedFile) return
    setInstalling(true)
    setInstallError(null)
    try {
      const addon = await serverApi.installAddon(serverId, selectedFile.url, projectType)
      onInstalled(addon)
      onClose()
    } catch (caught) {
      setInstallError(
        caught instanceof ApiError || caught instanceof Error
          ? caught.message
          : `Could not install ${hit.title}.`,
      )
    } finally {
      setInstalling(false)
    }
  }

  const description = project?.body?.trim() || project?.description || hit.description

  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-50 grid place-items-center bg-void/80 px-3 py-6 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="addon-detail-title"
        className="animate-rise flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-glass border border-white/10 bg-abyss/95 shadow-glass backdrop-blur-2xl"
      >
        <header className="flex items-start gap-4 border-b border-white/[0.07] px-6 py-5">
          <AddonIcon title={hit.title} iconUrl={project?.icon_url ?? hit.icon_url} />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h2 id="addon-detail-title" className="truncate text-lg font-semibold">
                {project?.title ?? hit.title}
              </h2>
              <span className="rounded border border-white/10 bg-white/5 px-1.5 py-px font-mono text-[8px] tracking-[0.1em] text-text-dim uppercase">
                {projectType}
              </span>
              {installedAddon ? (
                <span className="rounded border border-neon-emerald/30 bg-neon-emerald/10 px-1.5 py-px font-mono text-[8px] tracking-[0.1em] text-neon-emerald uppercase">
                  installed
                </span>
              ) : null}
            </div>
            <p className="mt-1 font-mono text-[10px] text-text-dim">
              by {hit.author || 'unknown'} · {formatDownloads(project?.downloads ?? hit.downloads)}{' '}
              downloads · {serverType} {gameVersion}
            </p>
          </div>
          <Button variant="ghost" size="sm" aria-label="Close" onClick={onClose}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" className="size-4">
              <path d="m6 6 12 12M18 6 6 18" strokeLinecap="round" />
            </svg>
          </Button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {loading ? (
            <p className="py-14 text-center font-mono text-[10px] tracking-[0.12em] text-text-dim uppercase">
              loading project…
            </p>
          ) : loadError ? (
            <ErrorBanner>{loadError}</ErrorBanner>
          ) : (
            <>
              <div className="flex flex-wrap gap-1.5">
                {(project?.categories ?? hit.categories).map((category) => (
                  <span
                    key={category}
                    className="rounded border border-white/10 bg-white/5 px-1.5 py-px font-mono text-[8px] tracking-[0.08em] text-text-dim uppercase"
                  >
                    {category}
                  </span>
                ))}
                {hit.slug ? (
                  <a
                    href={`https://modrinth.com/project/${hit.slug}`}
                    target="_blank"
                    rel="noreferrer"
                    className="rounded border border-neon-cyan/25 bg-neon-cyan/10 px-1.5 py-px font-mono text-[8px] tracking-[0.08em] text-neon-cyan uppercase transition hover:bg-neon-cyan/20"
                  >
                    open on modrinth ↗
                  </a>
                ) : null}
              </div>

              <div className="mt-4 border-t border-white/[0.06] pt-4">
                {description ? (
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={{
                      h1: ({ children }) => (
                        <h1 className="mt-5 mb-2 text-lg font-semibold text-text-primary">
                          {children}
                        </h1>
                      ),
                      h2: ({ children }) => (
                        <h2 className="mt-5 mb-2 text-base font-semibold text-text-primary">
                          {children}
                        </h2>
                      ),
                      h3: ({ children }) => (
                        <h3 className="mt-4 mb-1.5 text-sm font-semibold text-text-primary">
                          {children}
                        </h3>
                      ),
                      p: ({ children }) => (
                        <p className="my-2 text-xs leading-relaxed text-text-muted">{children}</p>
                      ),
                      a: ({ href, children }) => (
                        <a
                          href={href}
                          target="_blank"
                          rel="noreferrer"
                          className="text-neon-cyan underline decoration-neon-cyan/40 underline-offset-2 hover:decoration-neon-cyan"
                        >
                          {children}
                        </a>
                      ),
                      ul: ({ children }) => (
                        <ul className="my-2 list-disc space-y-1 pl-5 text-xs text-text-muted">
                          {children}
                        </ul>
                      ),
                      ol: ({ children }) => (
                        <ol className="my-2 list-decimal space-y-1 pl-5 text-xs text-text-muted">
                          {children}
                        </ol>
                      ),
                      li: ({ children }) => <li className="leading-relaxed">{children}</li>,
                      blockquote: ({ children }) => (
                        <blockquote className="my-3 border-l-2 border-neon-emerald/40 pl-3 text-xs text-text-dim italic">
                          {children}
                        </blockquote>
                      ),
                      code: ({ children }) => (
                        <code className="rounded bg-black/40 px-1 py-0.5 font-mono text-[10px] text-neon-amber">
                          {children}
                        </code>
                      ),
                      pre: ({ children }) => (
                        <pre className="my-3 overflow-x-auto rounded-lg border border-white/10 bg-black/40 p-3 font-mono text-[10px] text-text-muted">
                          {children}
                        </pre>
                      ),
                      img: ({ src, alt }) => (
                        <img
                          src={src}
                          alt={alt ?? ''}
                          loading="lazy"
                          className="my-3 max-h-72 max-w-full rounded-lg border border-white/10 object-contain"
                        />
                      ),
                      hr: () => <hr className="my-4 border-white/10" />,
                      table: ({ children }) => (
                        <div className="my-3 overflow-x-auto">
                          <table className="w-full border-collapse text-left text-[11px]">
                            {children}
                          </table>
                        </div>
                      ),
                      th: ({ children }) => (
                        <th className="border border-white/10 bg-white/5 px-2 py-1 font-semibold">
                          {children}
                        </th>
                      ),
                      td: ({ children }) => (
                        <td className="border border-white/10 px-2 py-1 text-text-muted">
                          {children}
                        </td>
                      ),
                    }}
                  >
                    {description}
                  </ReactMarkdown>
                ) : (
                  <p className="text-xs text-text-dim">No description provided.</p>
                )}
              </div>
            </>
          )}
        </div>

        <footer className="border-t border-white/[0.07] bg-black/20 px-6 py-4">
          {installedAddon ? (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="min-w-0 truncate font-mono text-[10px] text-text-dim">
                installed as {installedAddon.path}
              </p>
              <Button variant="danger" size="sm" onClick={() => onDelete(installedAddon)}>
                Delete
              </Button>
            </div>
          ) : (
            <div className="space-y-3">
              <div className="flex flex-wrap items-end gap-3">
                <label className="min-w-0 flex-1">
                  <span className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                    Loader / Software
                  </span>
                  <select
                    value={selectedLoader}
                    disabled={loading || loaderOptions.length === 0 || installing}
                    onChange={(event) => chooseLoader(event.target.value)}
                    className="mt-1.5 h-9 w-full appearance-none rounded-lg border border-white/10 bg-black/30 px-3 font-mono text-[10px] text-text-primary outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15 disabled:opacity-50"
                  >
                    {loaderOptions.length === 0 ? (
                      <option value="">No compatible loaders</option>
                    ) : (
                      loaderOptions.map((loader) => (
                        <option key={loader} value={loader}>
                          {loaderLabel(loader)}
                        </option>
                      ))
                    )}
                  </select>
                </label>
                <label className="min-w-0 flex-1">
                  <span className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                    Game Version
                  </span>
                  <select
                    value={selectedGameVersion}
                    disabled={
                      loading ||
                      gameVersionOptions.length === 0 ||
                      installing
                    }
                    onChange={(event) => setSelectedGameVersion(event.target.value)}
                    className="mt-1.5 h-9 w-full appearance-none rounded-lg border border-white/10 bg-black/30 px-3 font-mono text-[10px] text-text-primary outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15 disabled:opacity-50"
                  >
                    {gameVersionOptions.length === 0 ? (
                      <option value="">No compatible versions</option>
                    ) : (
                      gameVersionOptions.map((version) => (
                        <option key={version} value={version}>
                          {version}
                        </option>
                      ))
                    )}
                  </select>
                </label>
                <Button
                  variant="primary"
                  loading={installing}
                  disabled={!selectedFile || loading}
                  onClick={() => void installSelected()}
                  className="shrink-0"
                >
                  Install selected
                </Button>
              </div>

              {selectedVersion ? (
                <p className="font-mono text-[10px] text-text-dim">
                  Selected {selectedVersion.version_number} · {selectedVersion.version_type} ·{' '}
                  {selectedFile?.filename ?? 'file'}
                </p>
              ) : null}
              {installing && projectType === 'modpack' ? (
                <p
                  role="status"
                  aria-live="polite"
                  className="flex items-center gap-2 text-[11px] text-neon-amber"
                >
                  <Spinner className="size-3.5 text-neon-amber" />
                  Installing Modpack (This will take a few minutes)…
                </p>
              ) : null}
            </div>
          )}

          {installError ? (
            <p role="alert" className="mt-2 text-[11px] text-neon-rose">
              {installError}
            </p>
          ) : null}
          {!loading && gameVersionOptions.length === 0 ? (
            <p className="mt-2 text-[11px] text-neon-amber">
              No {projectType} release is tagged for {loaderLabel(selectedLoader)}{' '}
              {gameVersion}. Pick another loader or version, or try another project.
            </p>
          ) : null}
        </footer>
      </div>
    </div>,
    document.body,
  )
}

function AddonIcon({ title, iconUrl }: { title: string; iconUrl: string | null }) {
  const [failed, setFailed] = useState(false)
  return (
    <span className="grid size-11 shrink-0 place-items-center overflow-hidden rounded-lg border border-white/10 bg-gradient-to-br from-neon-emerald/20 to-neon-cyan/20 font-mono text-xs font-semibold text-text-primary">
      {iconUrl && !failed ? (
        <img
          src={iconUrl}
          alt=""
          loading="lazy"
          className="size-full object-cover"
          onError={() => setFailed(true)}
        />
      ) : (
        title.slice(0, 2).toUpperCase()
      )}
    </span>
  )
}

function defaultProjectType(serverType: ServerType): AddonProjectType {
  if (serverType === 'fabric' || serverType === 'forge') return 'mod'
  if (serverType === 'paper' || serverType === 'velocity' || serverType === 'bungeecord') {
    return 'plugin'
  }
  return 'datapack'
}

function supportsProjectType(projectType: AddonProjectType, serverType: ServerType): boolean {
  if (projectType === 'mod' || projectType === 'modpack') {
    return serverType === 'fabric' || serverType === 'forge'
  }
  if (projectType === 'plugin') {
    return serverType === 'paper' || serverType === 'velocity' || serverType === 'bungeecord'
  }
  return true
}

function projectTypeLabel(projectType: AddonProjectType): string {
  if (projectType === 'mod') return 'mods'
  if (projectType === 'plugin') return 'plugins'
  if (projectType === 'datapack') return 'datapacks'
  if (projectType === 'modpack') return 'modpacks'
  return 'resource packs'
}

function loaderCategory(serverType: ServerType, projectType: AddonProjectType): string | null {
  if (projectType === 'mod' || projectType === 'modpack') {
    if (serverType === 'fabric' || serverType === 'forge') return serverType
    return null
  }
  if (projectType === 'plugin') {
    if (serverType === 'paper') return 'paper'
    if (serverType === 'velocity') return 'velocity'
    if (serverType === 'bungeecord') return 'bungeecord'
    return null
  }
  return null
}

async function searchModrinth(
  query: string,
  projectType: AddonProjectType,
  serverType: ServerType,
): Promise<ModrinthHit[]> {
  const facets: string[][] = [[`project_type:${projectType}`]]
  const category = loaderCategory(serverType, projectType)
  if (category) facets.push([`categories:${category}`])

  const params = new URLSearchParams({
    query: query.trim(),
    limit: '20',
    index: 'relevance',
    facets: JSON.stringify(facets),
  })
  const payload = await modrinthJson<{ hits: ModrinthHit[] }>(`/search?${params.toString()}`)
  return payload.hits
}

const LOADER_ORDER = [
  'fabric',
  'forge',
  'neoforge',
  'quilt',
  'paper',
  'spigot',
  'bukkit',
  'purpur',
  'folia',
  'velocity',
  'bungeecord',
  'minecraft',
  'datapack',
]

/** Loaders the current server can actually run for this project type. */
function allowedLoaders(
  projectType: AddonProjectType,
  serverType: ServerType,
): Set<string> {
  if (projectType === 'modpack' || projectType === 'mod') {
    return new Set([serverType])
  }
  if (projectType === 'plugin') {
    if (serverType === 'paper') {
      return new Set(['paper', 'spigot', 'bukkit', 'purpur', 'folia'])
    }
    return new Set([serverType])
  }
  return new Set(['minecraft'])
}

function loaderOptionsFor(
  versions: ModrinthVersion[],
  projectType: AddonProjectType,
  serverType: ServerType,
): string[] {
  if (projectType === 'datapack' || projectType === 'resourcepack') {
    return ['minecraft']
  }
  const allowed = allowedLoaders(projectType, serverType)
  const found = new Set<string>()
  for (const version of versions) {
    if (!fileForVersion(version, projectType)) continue
    for (const loader of version.loaders) {
      const normalised = loader.toLowerCase()
      if (allowed.has(normalised)) found.add(normalised)
    }
  }
  return [...found].sort((left, right) => {
    const leftIndex = LOADER_ORDER.indexOf(left)
    const rightIndex = LOADER_ORDER.indexOf(right)
    if (leftIndex === -1 && rightIndex === -1) return left.localeCompare(right)
    if (leftIndex === -1) return 1
    if (rightIndex === -1) return -1
    return leftIndex - rightIndex
  })
}

function versionSupportsLoader(
  version: ModrinthVersion,
  loader: string,
  projectType: AddonProjectType,
): boolean {
  if (projectType === 'datapack' || projectType === 'resourcepack') return true
  return version.loaders.some((value) => value.toLowerCase() === loader)
}

/** Every game version actually tagged by the project for the chosen loader. */
function gameVersionOptionsFor(
  versions: ModrinthVersion[],
  loader: string,
  projectType: AddonProjectType,
): string[] {
  if (!loader) return []
  const found = new Set<string>()
  for (const version of versions) {
    if (!fileForVersion(version, projectType)) continue
    if (!versionSupportsLoader(version, loader, projectType)) continue
    for (const gameVersion of version.game_versions) {
      if (gameVersion.trim().length > 0) found.add(gameVersion)
    }
  }
  return [...found].sort((left, right) =>
    right.localeCompare(left, undefined, { numeric: true, sensitivity: 'base' }),
  )
}

function defaultLoaderOption(
  options: string[],
  projectType: AddonProjectType,
  serverType: ServerType,
): string {
  const preferred =
    projectType === 'datapack' || projectType === 'resourcepack'
      ? 'minecraft'
      : serverType
  return options.includes(preferred) ? preferred : (options[0] ?? '')
}

function defaultGameVersionOption(options: string[], serverGameVersion: string): string {
  return options.includes(serverGameVersion) ? serverGameVersion : (options[0] ?? '')
}

/** Resolve the exact Modrinth release for the chosen loader + game version. */
function resolveSelectedVersion(
  versions: ModrinthVersion[],
  loader: string,
  gameVersion: string,
  projectType: AddonProjectType,
): ModrinthVersion | null {
  if (!loader || !gameVersion) return null
  const matches = versions.filter(
    (version) =>
      fileForVersion(version, projectType) !== undefined &&
      versionSupportsLoader(version, loader, projectType) &&
      version.game_versions.includes(gameVersion),
  )
  return matches.find((version) => version.version_type === 'release') ?? matches[0] ?? null
}

function loaderLabel(loader: string): string {
  if (!loader) return 'Unknown'
  const known: Record<string, string> = {
    fabric: 'Fabric',
    forge: 'Forge',
    neoforge: 'NeoForge',
    quilt: 'Quilt',
    paper: 'Paper',
    spigot: 'Spigot',
    bukkit: 'Bukkit',
    purpur: 'Purpur',
    folia: 'Folia',
    velocity: 'Velocity',
    bungeecord: 'BungeeCord',
    minecraft: 'Minecraft',
    datapack: 'Datapack',
  }
  return known[loader] ?? loader.charAt(0).toUpperCase() + loader.slice(1)
}

function compatibleVersions(
  versions: ModrinthVersion[],
  projectType: AddonProjectType,
  serverType: ServerType,
  gameVersion: string,
): ModrinthVersion[] {
  return versions.filter(
    (version) =>
      isLoaderCompatible(version, projectType, serverType) &&
      isGameVersionCompatible(version, gameVersion) &&
      fileForVersion(version, projectType) !== undefined,
  )
}

function isGameVersionCompatible(version: ModrinthVersion, gameVersion: string): boolean {
  const target = gameVersion.trim()
  if (target.length === 0) return true
  const upper = target.toUpperCase()
  if (upper === 'LATEST' || upper === 'SNAPSHOT') return true
  return version.game_versions.some((value) => value === target)
}

function isLoaderCompatible(
  version: ModrinthVersion,
  projectType: AddonProjectType,
  serverType: ServerType,
): boolean {
  const loaders = version.loaders.map((loader) => loader.toLowerCase())
  if (projectType === 'mod' || projectType === 'modpack') return loaders.includes(serverType)
  if (projectType === 'plugin') {
    if (serverType === 'paper') {
      return loaders.some((loader) =>
        ['paper', 'spigot', 'bukkit', 'purpur', 'folia'].includes(loader),
      )
    }
    return loaders.includes(serverType)
  }
  return true
}

function hasExtension(filename: string, projectType: AddonProjectType): boolean {
  const lower = filename.toLowerCase()
  if (projectType === 'mod' || projectType === 'plugin') return lower.endsWith('.jar')
  if (projectType === 'modpack') return lower.endsWith('.mrpack')
  return lower.endsWith('.zip')
}

function fileForVersion(
  version: ModrinthVersion,
  projectType: AddonProjectType,
): ModrinthVersionFile | undefined {
  const files = version.files.filter((file) => hasExtension(file.filename, projectType))
  return files.find((candidate) => candidate.primary) ?? files[0]
}

async function latestVersion(
  projectId: string,
  projectType: AddonProjectType,
  serverType: ServerType,
  gameVersion: string,
): Promise<{ url: string; filename: string; version: string }> {
  const versions = await modrinthJson<ModrinthVersion[]>(
    `/project/${encodeURIComponent(projectId)}/version`,
  )
  const compatible = compatibleVersions(versions, projectType, serverType, gameVersion)
  const releases = compatible.filter((version) => version.version_type === 'release')
  const pool = releases.length > 0 ? releases : compatible

  for (const version of pool) {
    const file = fileForVersion(version, projectType)
    if (file) return { url: file.url, filename: file.filename, version: version.version_number }
  }

  throw new Error(
    `No ${projectType} file compatible with ${serverType} ${gameVersion} was found for this project.`,
  )
}

function findInstalledForHit(
  hit: ModrinthHit,
  installed: InstalledAddon[],
  projectType: AddonProjectType,
  projectFiles: Record<string, string>,
): InstalledAddon | null {
  const candidates = installed.filter((addon) => addon.project_type === projectType)
  if (candidates.length === 0) return null

  const mapped = projectFiles[hit.project_id]
  if (mapped) {
    const exact = candidates.find((addon) => addon.name === mapped || addon.path === mapped)
    if (exact) return exact
  }

  const slug = normalise(hit.slug)
  const title = normalise(hit.title)
  return (
    candidates.find((addon) => {
      const name = normalise(addon.name)
      return (
        (slug.length >= 3 && name.includes(slug)) ||
        (title.length >= 4 && name.includes(title))
      )
    }) ?? null
  )
}

function normalise(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, '')
}

function readProjectFiles(serverId: string): Record<string, string> {
  try {
    const raw = window.localStorage.getItem(`${PROJECT_STORAGE_PREFIX}${serverId}`)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return {}
    return Object.fromEntries(
      Object.entries(parsed as Record<string, unknown>).filter(
        (entry): entry is [string, string] => typeof entry[1] === 'string',
      ),
    )
  } catch {
    return {}
  }
}

function saveProjectFiles(serverId: string, files: Record<string, string>): void {
  try {
    window.localStorage.setItem(`${PROJECT_STORAGE_PREFIX}${serverId}`, JSON.stringify(files))
  } catch {
    // Storage may be unavailable; matching by filename still works.
  }
}

async function modrinthJson<T>(path: string): Promise<T> {
  let response: Response
  try {
    response = await fetch(`${MODRINTH_API}${path}`, {
      headers: { accept: 'application/json' },
    })
  } catch {
    throw new Error('Could not reach the Modrinth API. Check your network connection.')
  }
  if (!response.ok) {
    throw new Error(`Modrinth request failed (${response.status}).`)
  }
  return (await response.json()) as T
}

function formatDownloads(downloads: number): string {
  if (downloads >= 1_000_000) return `${(downloads / 1_000_000).toFixed(1)}M`
  if (downloads >= 1_000) return `${(downloads / 1_000).toFixed(1)}K`
  return downloads.toString()
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${bytes} B`
}
