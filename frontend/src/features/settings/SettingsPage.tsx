import { useAuth } from '../../auth/auth-context'
import { Button } from '../../components/ui/Button'
import { GlassCard } from '../../components/ui/GlassCard'
import { PageHeader } from '../../components/ui/PageHeader'

const PALETTE = [
  { name: 'emerald', className: 'bg-neon-emerald' },
  { name: 'cyan', className: 'bg-neon-cyan' },
  { name: 'violet', className: 'bg-neon-violet' },
  { name: 'amber', className: 'bg-neon-amber' },
  { name: 'rose', className: 'bg-neon-rose' },
]

export function SettingsPage() {
  const { user, logout } = useAuth()

  return (
    <>
      <PageHeader
        eyebrow="configuration"
        title="Settings"
        description="Session and node details. Server-level configuration lands with the visual editor milestone."
      />

      <div className="grid gap-4 lg:grid-cols-2">
        <GlassCard className="animate-rise p-6">
          <h2 className="text-sm font-semibold">Account</h2>
          <p className="mt-1 text-xs text-text-muted">The principal behind the current bearer token.</p>

          <dl className="mt-5 space-y-3 text-xs">
            <div className="flex items-center justify-between gap-4 border-b border-white/[0.06] pb-3">
              <dt className="text-text-dim">Username</dt>
              <dd className="font-medium">{user?.username ?? '—'}</dd>
            </div>
            <div className="flex items-center justify-between gap-4 border-b border-white/[0.06] pb-3">
              <dt className="text-text-dim">User ID</dt>
              <dd className="max-w-[60%] truncate font-mono text-[10px] text-text-muted" title={user?.id}>
                {user?.id ?? '—'}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-4">
              <dt className="text-text-dim">Password state</dt>
              <dd className="font-mono text-[10px] text-neon-emerald">rotated</dd>
            </div>
          </dl>

          <Button variant="danger" size="sm" className="mt-6" onClick={logout}>
            Sign out of this browser
          </Button>
        </GlassCard>

        <GlassCard className="animate-rise p-6">
          <h2 className="text-sm font-semibold">Session transport</h2>
          <p className="mt-1 text-xs text-text-muted">How the SPA talks to the control plane.</p>

          <ul className="mt-5 space-y-3 text-xs">
            <SettingRow label="API base" value="same-origin /api" />
            <SettingRow label="Credentials" value="Authorization: Bearer" />
            <SettingRow label="Storage" value="localStorage · cluster.token" />
            <SettingRow label="Expiry handling" value="401 → signed out" />
            <SettingRow label="Password gate" value="403 password_change_required" />
          </ul>

          <p className="mt-5 rounded-xl border border-neon-amber/20 bg-neon-amber/[0.06] px-3.5 py-3 text-[11px] leading-relaxed text-neon-amber/90">
            localStorage is appropriate for this single-origin admin console. A hardened deployment
            can move the token into an HttpOnly cookie without changing the screens.
          </p>
        </GlassCard>

        <GlassCard className="animate-rise p-6 lg:col-span-2">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold">Aurora Glass</h2>
              <p className="mt-1 text-xs text-text-muted">
                Theme tokens defined in <span className="font-mono">src/index.css</span>.
              </p>
            </div>
            <span className="font-mono text-[10px] tracking-[0.18em] text-text-dim uppercase">
              M3 · glassmorphic foundation
            </span>
          </div>

          <div className="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-5">
            {PALETTE.map((swatch) => (
              <div
                key={swatch.name}
                className="rounded-xl border border-white/[0.07] bg-black/20 p-3"
              >
                <span className={`block h-10 rounded-lg ${swatch.className}`} />
                <span className="mt-2 block font-mono text-[10px] text-text-muted">
                  {swatch.name}
                </span>
              </div>
            ))}
          </div>
        </GlassCard>
      </div>
    </>
  )
}

function SettingRow({ label, value }: { label: string; value: string }) {
  return (
    <li className="flex items-center justify-between gap-4 border-b border-white/[0.06] pb-3 last:border-0 last:pb-0">
      <span className="text-text-dim">{label}</span>
      <span className="text-right font-mono text-[10px] text-text-muted">{value}</span>
    </li>
  )
}
