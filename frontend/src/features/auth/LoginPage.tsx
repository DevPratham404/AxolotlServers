import { useState } from 'react'
import type { FormEvent } from 'react'

import { useAuth } from '../../auth/auth-context'
import { AuroraBackdrop } from '../../components/layout/AuroraBackdrop'
import { Brand } from '../../components/ui/Brand'
import { Button } from '../../components/ui/Button'
import { ErrorBanner } from '../../components/ui/Feedback'
import { TextField } from '../../components/ui/TextField'
import { ApiError } from '../../lib/api'

const HIGHLIGHTS = [
  {
    title: 'One console, every world',
    body: 'Create, start and stop Paper, Vanilla, Fabric and proxy servers without a shell.',
  },
  {
    title: 'Limits the runtime enforces',
    body: 'RAM and CPU are hard Docker limits — not polite requests the JVM can ignore.',
  },
  {
    title: 'HMAC-signed control plane',
    body: 'The web process never holds the Docker socket. Every daemon call is signed.',
  },
]

export function LoginPage() {
  const { login } = useAuth()
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [showPassword, setShowPassword] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      await login(username.trim(), password)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Sign-in failed. Try again.')
      setBusy(false)
    }
  }

  return (
    <div className="relative min-h-dvh">
      <AuroraBackdrop />

      <div className="mx-auto grid min-h-dvh w-full max-w-6xl items-center gap-12 px-6 py-12 lg:grid-cols-[1.1fr_minmax(380px,440px)] lg:gap-16 lg:px-10">
        <section className="animate-rise hidden lg:block">
          <Brand />
          <p className="mt-12 font-mono text-[11px] tracking-[0.3em] text-neon-emerald uppercase">
            Aurora Glass · v1
          </p>
          <h1 className="mt-4 max-w-lg text-5xl leading-[1.05] font-semibold tracking-tight">
            Your cluster,
            <br />
            <span className="text-aurora">rendered in glass.</span>
          </h1>
          <p className="mt-5 max-w-md text-sm leading-relaxed text-text-muted">
            A single dashboard for the Minecraft servers on this node. No terminals, no forgotten
            screen sessions — just typed operations with visible state.
          </p>

          <ul className="mt-10 space-y-3">
            {HIGHLIGHTS.map((item, index) => (
              <li key={item.title} className="glass flex gap-4 rounded-2xl p-4">
                <span className="mt-0.5 font-mono text-[10px] text-text-dim">
                  0{index + 1}
                </span>
                <span>
                  <span className="block text-sm font-medium">{item.title}</span>
                  <span className="mt-1 block text-xs leading-relaxed text-text-muted">
                    {item.body}
                  </span>
                </span>
              </li>
            ))}
          </ul>
        </section>

        <section className="animate-rise mx-auto w-full max-w-md">
          <div className="glass-deep rounded-glass p-7 shadow-glass sm:p-9">
            <div className="mb-8 flex items-center justify-between">
              <div className="lg:hidden">
                <Brand compact />
              </div>
              <span className="font-mono text-[10px] tracking-[0.22em] text-text-dim uppercase">
                secure sign-in
              </span>
            </div>

            <h2 className="text-2xl font-semibold tracking-tight">Welcome back</h2>
            <p className="mt-1.5 text-sm text-text-muted">
              Authenticate to reach the control plane.
            </p>

            <form className="mt-7 space-y-4" onSubmit={onSubmit}>
              {error ? <ErrorBanner>{error}</ErrorBanner> : null}

              <TextField
                label="Username"
                name="username"
                autoComplete="username"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
                placeholder="admin"
                required
                autoFocus
              />

              <TextField
                label="Password"
                name="password"
                type={showPassword ? 'text' : 'password'}
                autoComplete="current-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                placeholder="••••••••"
                required
                trailing={
                  <button
                    type="button"
                    onClick={() => setShowPassword((visible) => !visible)}
                    className="text-[10px] font-medium tracking-wide text-text-dim transition hover:text-text-muted"
                    aria-label={showPassword ? 'Hide password' : 'Show password'}
                  >
                    {showPassword ? 'HIDE' : 'SHOW'}
                  </button>
                }
              />

              <Button
                type="submit"
                variant="primary"
                size="lg"
                className="mt-2 w-full"
                loading={busy}
                disabled={!username.trim() || !password}
              >
                Enter the console
              </Button>
            </form>

            <p className="mt-6 text-center text-[11px] leading-relaxed text-text-dim">
              First login requires a password change. Sessions are bearer JWTs stored in this
              browser.
            </p>
          </div>
        </section>
      </div>
    </div>
  )
}
