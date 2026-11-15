import { useState } from 'react'
import type { FormEvent } from 'react'

import { useAuth } from '../../auth/auth-context'
import { AuroraBackdrop } from '../../components/layout/AuroraBackdrop'
import { Brand } from '../../components/ui/Brand'
import { Button } from '../../components/ui/Button'
import { ErrorBanner } from '../../components/ui/Feedback'
import { TextField } from '../../components/ui/TextField'
import { ApiError } from '../../lib/api'

const MIN_PASSWORD_LENGTH = 8

export function ChangePasswordPage() {
  const { user, changePassword, logout } = useAuth()
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const tooShort = newPassword.length > 0 && newPassword.length < MIN_PASSWORD_LENGTH
  const mismatch = confirmPassword.length > 0 && confirmPassword !== newPassword
  const same = newPassword.length > 0 && newPassword === currentPassword
  const canSubmit =
    currentPassword.length > 0 &&
    newPassword.length >= MIN_PASSWORD_LENGTH &&
    !mismatch &&
    !same &&
    !busy

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!canSubmit) return
    setBusy(true)
    setError(null)
    try {
      await changePassword(currentPassword, newPassword)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not change the password.')
      setBusy(false)
    }
  }

  return (
    <div className="relative grid min-h-dvh place-items-center px-6 py-12">
      <AuroraBackdrop />

      <div className="animate-rise w-full max-w-lg">
        <div className="mb-8 flex justify-center">
          <Brand />
        </div>

        <div className="glass-deep rounded-glass p-7 shadow-glass sm:p-9">
          <span className="inline-flex items-center gap-2 rounded-full border border-neon-amber/30 bg-neon-amber/10 px-3 py-1 font-mono text-[10px] tracking-[0.16em] text-neon-amber uppercase">
            <span className="size-1.5 rounded-full bg-neon-amber" />
            action required
          </span>

          <h1 className="mt-5 text-2xl font-semibold tracking-tight">Rotate your password</h1>
          <p className="mt-2 text-sm leading-relaxed text-text-muted">
            <span className="font-medium text-text-primary">{user?.username}</span> is still using
            the bootstrap credential. Server controls stay locked until this password is replaced.
          </p>

          <form className="mt-7 space-y-4" onSubmit={onSubmit}>
            {error ? <ErrorBanner>{error}</ErrorBanner> : null}

            <TextField
              label="Current password"
              type="password"
              autoComplete="current-password"
              value={currentPassword}
              onChange={(event) => setCurrentPassword(event.target.value)}
              placeholder="The password you just signed in with"
              required
              autoFocus
            />

            <TextField
              label="New password"
              type="password"
              autoComplete="new-password"
              value={newPassword}
              onChange={(event) => setNewPassword(event.target.value)}
              placeholder={`At least ${MIN_PASSWORD_LENGTH} characters`}
              required
              error={tooShort ? `Use at least ${MIN_PASSWORD_LENGTH} characters.` : undefined}
              hint={
                tooShort ? undefined : `${newPassword.length} / ${MIN_PASSWORD_LENGTH} minimum`
              }
            />

            <TextField
              label="Confirm new password"
              type="password"
              autoComplete="new-password"
              value={confirmPassword}
              onChange={(event) => setConfirmPassword(event.target.value)}
              placeholder="Repeat the new password"
              required
              error={mismatch ? 'Passwords do not match.' : undefined}
            />

            {same ? (
              <p className="text-xs text-neon-amber">The new password must differ from the current one.</p>
            ) : null}

            <Button
              type="submit"
              variant="primary"
              size="lg"
              className="w-full"
              loading={busy}
              disabled={!canSubmit}
            >
              Save and continue
            </Button>
          </form>

          <div className="mt-6 flex items-center justify-between border-t border-white/[0.07] pt-5">
            <span className="text-[11px] text-text-dim">
              Token rotates automatically after the change.
            </span>
            <button
              type="button"
              onClick={logout}
              className="text-[11px] font-medium text-text-muted transition hover:text-neon-rose"
            >
              Sign out
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
