import { useCallback, useEffect, useState, type FormEvent } from 'react'

import { ApiError, serverApi } from '../../lib/api'
import type {
  BackupInfo,
  ScheduleTaskType,
  ScheduleView,
  ServerStatus,
} from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { EmptyState, ErrorBanner } from '../../components/ui/Feedback'
import { GlassCard } from '../../components/ui/GlassCard'
import { TextField } from '../../components/ui/TextField'

const TASK_TYPES: Array<{ value: ScheduleTaskType; label: string; hint: string }> = [
  { value: 'backup', label: 'Backup', hint: 'snapshot the whole data volume' },
  { value: 'restart', label: 'Restart', hint: 'graceful stop then start' },
  { value: 'start', label: 'Start', hint: 'start the container' },
  { value: 'stop', label: 'Stop', hint: 'graceful stop' },
  { value: 'command', label: 'Command', hint: 'run an RCON command' },
]

const INTERVAL_OPTIONS = [
  { label: 'Every 15 minutes', value: 15 },
  { label: 'Every hour', value: 60 },
  { label: 'Every 6 hours', value: 360 },
  { label: 'Every 12 hours', value: 720 },
  { label: 'Daily', value: 1440 },
  { label: 'Weekly', value: 10080 },
]

export function AutomationPanel({
  serverId,
  status,
  onStatusChanged,
}: {
  serverId: string
  status: ServerStatus
  onStatusChanged?: () => void
}) {
  const [backups, setBackups] = useState<BackupInfo[]>([])
  const [schedules, setSchedules] = useState<ScheduleView[]>([])
  const [retentionCount, setRetentionCount] = useState(0)
  const [retentionInput, setRetentionInput] = useState('0')
  const [verified, setVerified] = useState<Record<string, boolean>>({})
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [backupName, setBackupName] = useState('')
  const [taskName, setTaskName] = useState('Nightly backup')
  const [taskType, setTaskType] = useState<ScheduleTaskType>('backup')
  const [interval, setInterval] = useState(1440)
  const [command, setCommand] = useState('')

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [nextBackups, nextSchedules, settings] = await Promise.all([
        serverApi.listBackups(serverId),
        serverApi.listSchedules(serverId),
        serverApi.getBackupSettings(serverId),
      ])
      setBackups(nextBackups)
      setSchedules(nextSchedules)
      setRetentionCount(settings.retention_count)
      setRetentionInput(String(settings.retention_count))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not load automation data.')
    } finally {
      setLoading(false)
    }
  }, [serverId])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    void refresh()
  }, [refresh])

  async function createBackup() {
    setBusy('backup')
    setError(null)
    setNotice(null)
    try {
      const backup = await serverApi.createBackup(serverId, backupName.trim() || undefined)
      setBackups((current) => [backup, ...current])
      setBackupName('')
      setNotice(`Created ${backup.name}.`)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not create a backup.')
    } finally {
      setBusy(null)
    }
  }

  async function restoreBackup(backup: BackupInfo) {
    if (
      !window.confirm(
        `Restore “${backup.name}”? The current data volume is replaced. A safety backup is recommended first.`,
      )
    ) {
      return
    }
    setBusy(`restore-${backup.id}`)
    setError(null)
    setNotice(null)
    try {
      await serverApi.restoreBackup(serverId, backup.id)
      setNotice(`Restored ${backup.name}.`)
      onStatusChanged?.()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not restore the backup.')
    } finally {
      setBusy(null)
    }
  }

  async function deleteBackup(backup: BackupInfo) {
    if (!window.confirm(`Delete “${backup.name}”? This cannot be undone.`)) return
    setBusy(`delete-${backup.id}`)
    setError(null)
    try {
      await serverApi.deleteBackup(serverId, backup.id)
      setBackups((current) => current.filter((item) => item.id !== backup.id))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not delete the backup.')
    } finally {
      setBusy(null)
    }
  }

  async function downloadBackup(backup: BackupInfo) {
    setBusy(`download-${backup.id}`)
    setError(null)
    try {
      const blob = await serverApi.downloadBackup(serverId, backup.id)
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `backup-${backup.id}.zip`
      document.body.appendChild(anchor)
      anchor.click()
      anchor.remove()
      URL.revokeObjectURL(url)
      setNotice(`Downloaded ${backup.name}.`)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not download the backup.')
    } finally {
      setBusy(null)
    }
  }

  async function verifyBackup(backup: BackupInfo) {
    setBusy(`verify-${backup.id}`)
    setError(null)
    setNotice(null)
    try {
      const result = await serverApi.verifyBackup(serverId, backup.id)
      setVerified((current) => ({ ...current, [backup.id]: result.verified }))
      if (result.verified) {
        setNotice(`${backup.name} matches its SHA-256 manifest.`)
      } else {
        setError(`${backup.name} is corrupt: the archive no longer matches its manifest.`)
      }
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not verify the backup.')
    } finally {
      setBusy(null)
    }
  }

  async function saveRetention() {
    const next = Number(retentionInput)
    if (!Number.isInteger(next) || next < 0 || next > 500) {
      setError('Retention must be a whole number between 0 and 500.')
      return
    }
    setBusy('retention')
    setError(null)
    setNotice(null)
    try {
      const settings = await serverApi.updateBackupSettings(serverId, next)
      setRetentionCount(settings.retention_count)
      setRetentionInput(String(settings.retention_count))
      setNotice(
        settings.retention_count === 0
          ? 'Retention disabled: every backup is kept.'
          : `Keeping the ${settings.retention_count} newest backups; older archives are pruned after the next backup.`,
      )
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save retention.')
    } finally {
      setBusy(null)
    }
  }

  async function createSchedule(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy('schedule')
    setError(null)
    setNotice(null)
    try {
      const schedule = await serverApi.createSchedule(serverId, {
        name: taskName,
        task_type: taskType,
        interval_minutes: interval,
        command: taskType === 'command' ? command : undefined,
      })
      setSchedules((current) => [...current, schedule])
      setNotice(`Scheduled “${schedule.name}”.`)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not create the schedule.')
    } finally {
      setBusy(null)
    }
  }

  async function toggleSchedule(schedule: ScheduleView) {
    setBusy(`toggle-${schedule.id}`)
    setError(null)
    try {
      const updated = await serverApi.updateSchedule(serverId, schedule.id, {
        enabled: !schedule.enabled,
      })
      setSchedules((current) =>
        current.map((item) => (item.id === updated.id ? updated : item)),
      )
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update the schedule.')
    } finally {
      setBusy(null)
    }
  }

  async function runSchedule(schedule: ScheduleView) {
    setBusy(`run-${schedule.id}`)
    setError(null)
    setNotice(null)
    try {
      const updated = await serverApi.runSchedule(serverId, schedule.id)
      setSchedules((current) =>
        current.map((item) => (item.id === updated.id ? updated : item)),
      )
      setNotice(`Ran “${schedule.name}”: ${updated.last_status ?? 'done'}.`)
      onStatusChanged?.()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not run the schedule.')
    } finally {
      setBusy(null)
    }
  }

  async function deleteSchedule(schedule: ScheduleView) {
    if (!window.confirm(`Delete the schedule “${schedule.name}”?`)) return
    setBusy(`delete-schedule-${schedule.id}`)
    try {
      await serverApi.deleteSchedule(serverId, schedule.id)
      setSchedules((current) => current.filter((item) => item.id !== schedule.id))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not delete the schedule.')
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="space-y-4">
      {error ? <ErrorBanner>{error}</ErrorBanner> : null}
      {notice ? (
        <p className="rounded-xl border border-neon-emerald/20 bg-neon-emerald/[0.06] px-4 py-2.5 text-[11px] text-neon-emerald">
          {notice}
        </p>
      ) : null}

      <GlassCard className="animate-rise p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold">Backups</h2>
            <p className="mt-1 text-[11px] text-text-muted">
              ZIP snapshots of <span className="font-mono">/data</span>. Running servers are
              flushed with RCON before the archive is written. Container is currently{' '}
              <span className="font-mono">{status}</span>.
            </p>
          </div>
          <Button variant="ghost" size="sm" loading={loading} onClick={() => void refresh()}>
            Refresh
          </Button>
        </div>

        <div className="mt-4 flex flex-wrap items-end gap-3">
          <label className="min-w-48 flex-1">
            <span className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
              Backup name
            </span>
            <input
              value={backupName}
              onChange={(event) => setBackupName(event.target.value)}
              placeholder="e.g. before 1.21 update"
              className="mt-1.5 h-9 w-full rounded-lg border border-white/10 bg-black/30 px-3 text-sm outline-none focus:border-neon-emerald/50"
            />
          </label>
          <Button
            variant="primary"
            loading={busy === 'backup'}
            disabled={busy !== null}
            onClick={() => void createBackup()}
          >
            Create backup
          </Button>
        </div>

        <div className="mt-4 flex flex-wrap items-end gap-3 rounded-xl border border-white/[0.07] bg-black/20 px-4 py-3">
          <label className="w-40">
            <span className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
              Keep newest
            </span>
            <input
              type="number"
              min={0}
              max={500}
              value={retentionInput}
              onChange={(event) => setRetentionInput(event.target.value)}
              className="mt-1.5 h-9 w-full rounded-lg border border-white/10 bg-black/30 px-3 text-sm outline-none focus:border-neon-emerald/50"
            />
          </label>
          <div className="min-w-48 flex-1 text-[11px] text-text-muted">
            <p>
              {retentionCount === 0
                ? 'Retention is off: every backup is kept.'
                : `Oldest archives are pruned after each backup, keeping ${retentionCount}.`}
            </p>
            <p className="mt-0.5 text-text-dim">Use 0 to keep every snapshot. Maximum 500.</p>
          </div>
          <Button
            size="sm"
            variant="secondary"
            loading={busy === 'retention'}
            disabled={busy !== null}
            onClick={() => void saveRetention()}
          >
            Save retention
          </Button>
        </div>

        {backups.length === 0 ? (
          <p className="mt-4 rounded-xl border border-white/[0.07] bg-black/20 px-4 py-5 text-center text-[11px] text-text-dim">
            {loading ? 'Loading backups…' : 'No backups yet. Create one before risky changes.'}
          </p>
        ) : (
          <ul className="mt-4 divide-y divide-white/[0.06] overflow-hidden rounded-xl border border-white/[0.07] bg-black/20">
            {backups.map((backup) => (
              <li key={backup.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-xs font-medium">{backup.name}</p>
                  <p className="mt-0.5 font-mono text-[10px] text-text-dim">
                    {formatDateMs(backup.created_at_ms)} · {formatBytes(backup.size_bytes)} ·{' '}
                    {backup.server_was_running ? 'live snapshot' : 'stopped snapshot'}
                    {verified[backup.id] === true ? (
                      <span className="ml-1.5 text-neon-emerald">· verified</span>
                    ) : verified[backup.id] === false ? (
                      <span className="ml-1.5 text-neon-rose">· corrupt</span>
                    ) : null}
                  </p>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  loading={busy === `verify-${backup.id}`}
                  disabled={busy !== null}
                  onClick={() => void verifyBackup(backup)}
                >
                  Verify
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  loading={busy === `download-${backup.id}`}
                  disabled={busy !== null}
                  onClick={() => void downloadBackup(backup)}
                >
                  Download
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  loading={busy === `restore-${backup.id}`}
                  disabled={busy !== null}
                  onClick={() => void restoreBackup(backup)}
                >
                  Restore
                </Button>
                <Button
                  size="sm"
                  variant="danger"
                  loading={busy === `delete-${backup.id}`}
                  disabled={busy !== null}
                  onClick={() => void deleteBackup(backup)}
                >
                  Delete
                </Button>
              </li>
            ))}
          </ul>
        )}
      </GlassCard>

      <GlassCard className="animate-rise p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold">Scheduled tasks</h2>
            <p className="mt-1 text-[11px] text-text-muted">
              Recurring backups, power actions and RCON commands. The worker checks every 20
              seconds and records the last result.
            </p>
          </div>
        </div>

        <form onSubmit={(event) => void createSchedule(event)} className="mt-4 space-y-3">
          <div className="grid gap-3 md:grid-cols-3">
            <TextField
              label="Name"
              value={taskName}
              onChange={(event) => setTaskName(event.target.value)}
              required
            />
            <label className="block text-sm font-medium">
              Task
              <select
                value={taskType}
                onChange={(event) => setTaskType(event.target.value as ScheduleTaskType)}
                className="mt-2 w-full rounded-xl border border-white/10 bg-black/30 px-3 py-2.5 text-sm"
              >
                {TASK_TYPES.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label} — {option.hint}
                  </option>
                ))}
              </select>
            </label>
            <label className="block text-sm font-medium">
              Frequency
              <select
                value={interval}
                onChange={(event) => setInterval(Number(event.target.value))}
                className="mt-2 w-full rounded-xl border border-white/10 bg-black/30 px-3 py-2.5 text-sm"
              >
                {INTERVAL_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {taskType === 'command' ? (
            <TextField
              label="RCON command"
              value={command}
              onChange={(event) => setCommand(event.target.value)}
              placeholder="e.g. say Backing up in 5 minutes"
              hint="Leading slash optional. Control characters are rejected."
              required
            />
          ) : null}
          <Button type="submit" variant="primary" loading={busy === 'schedule'} disabled={busy !== null}>
            Add schedule
          </Button>
        </form>

        {schedules.length === 0 ? (
          <div className="mt-4">
            <EmptyState
              title="No scheduled tasks"
              description="Add a recurring backup or restart so routine operations do not depend on memory."
            />
          </div>
        ) : (
          <ul className="mt-4 divide-y divide-white/[0.06] overflow-hidden rounded-xl border border-white/[0.07] bg-black/20">
            {schedules.map((schedule) => (
              <li key={schedule.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="truncate text-xs font-medium">{schedule.name}</p>
                    <span className="rounded border border-neon-cyan/25 bg-neon-cyan/10 px-1.5 py-px font-mono text-[8px] tracking-[0.1em] text-neon-cyan uppercase">
                      {schedule.task_type}
                    </span>
                    <span
                      className={
                        schedule.enabled
                          ? 'rounded border border-neon-emerald/30 bg-neon-emerald/10 px-1.5 py-px font-mono text-[8px] uppercase text-neon-emerald'
                          : 'rounded border border-white/10 bg-white/5 px-1.5 py-px font-mono text-[8px] uppercase text-text-dim'
                      }
                    >
                      {schedule.enabled ? 'enabled' : 'paused'}
                    </span>
                  </div>
                  <p className="mt-0.5 font-mono text-[10px] text-text-dim">
                    every {formatInterval(schedule.interval_minutes)} · next{' '}
                    {formatIso(schedule.next_run_at)}
                    {schedule.last_run_at ? ` · last ${formatIso(schedule.last_run_at)}` : ''}
                    {schedule.last_status ? ` (${schedule.last_status})` : ''}
                  </p>
                  {schedule.command ? (
                    <p className="mt-0.5 truncate font-mono text-[10px] text-text-muted">
                      /{schedule.command}
                    </p>
                  ) : null}
                  {schedule.last_error ? (
                    <p className="mt-0.5 truncate text-[10px] text-neon-rose">{schedule.last_error}</p>
                  ) : null}
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  loading={busy === `run-${schedule.id}`}
                  disabled={busy !== null}
                  onClick={() => void runSchedule(schedule)}
                >
                  Run now
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  loading={busy === `toggle-${schedule.id}`}
                  disabled={busy !== null}
                  onClick={() => void toggleSchedule(schedule)}
                >
                  {schedule.enabled ? 'Pause' : 'Enable'}
                </Button>
                <Button
                  size="sm"
                  variant="danger"
                  loading={busy === `delete-schedule-${schedule.id}`}
                  disabled={busy !== null}
                  onClick={() => void deleteSchedule(schedule)}
                >
                  Delete
                </Button>
              </li>
            ))}
          </ul>
        )}
      </GlassCard>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${bytes} B`
}

function formatDateMs(value: number): string {
  return formatIso(new Date(value).toISOString())
}

function formatIso(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '—'
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}

function formatInterval(minutes: number): string {
  if (minutes % 10080 === 0) return `${minutes / 10080}w`
  if (minutes % 1440 === 0) return `${minutes / 1440}d`
  if (minutes % 60 === 0) return `${minutes / 60}h`
  return `${minutes}m`
}
