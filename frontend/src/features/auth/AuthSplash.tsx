import { Brand } from '../../components/ui/Brand'
import { Spinner } from '../../components/ui/Spinner'
import { AuroraBackdrop } from '../../components/layout/AuroraBackdrop'

export function AuthSplash() {
  return (
    <div className="relative grid min-h-dvh place-items-center p-8">
      <AuroraBackdrop />
      <div className="glass-deep animate-rise flex flex-col items-center gap-5 rounded-glass px-10 py-9 text-center shadow-glass">
        <Brand className="justify-center" />
        <span className="flex items-center gap-2.5 text-xs text-text-muted">
          <Spinner className="size-4 text-neon-emerald" />
          Restoring session…
        </span>
      </div>
    </div>
  )
}
