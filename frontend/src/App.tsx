import { useState } from 'react'

import { AuthProvider } from './auth/AuthProvider'
import { useAuth } from './auth/auth-context'
import { AppShell } from './components/layout/AppShell'
import type { View } from './components/layout/Sidebar'
import { AuthSplash } from './features/auth/AuthSplash'
import { ChangePasswordPage } from './features/auth/ChangePasswordPage'
import { LoginPage } from './features/auth/LoginPage'
import { DashboardPage } from './features/dashboard/DashboardPage'
import { ServerDetailPage } from './features/server-detail/ServerDetailPage'
import { ServersPage } from './features/servers/ServersPage'
import { SettingsPage } from './features/settings/SettingsPage'

export default function App() {
  return (
    <AuthProvider>
      <Root />
    </AuthProvider>
  )
}

function Root() {
  const { status, user, logout } = useAuth()
  const [view, setView] = useState<View>('dashboard')
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null)

  if (status === 'booting') return <AuthSplash />
  if (!user) return <LoginPage />
  if (user.requires_password_change) return <ChangePasswordPage />

  function navigate(next: View) {
    setSelectedServerId(null)
    setView(next)
  }

  return (
    <AppShell view={view} onNavigate={navigate} username={user.username} onLogout={logout}>
      {selectedServerId ? (
        <ServerDetailPage
          key={selectedServerId}
          serverId={selectedServerId}
          onBack={() => setSelectedServerId(null)}
        />
      ) : (
        <>
          {view === 'dashboard' ? (
            <DashboardPage onOpenServers={() => setView('servers')} />
          ) : null}
          {view === 'servers' ? (
            <ServersPage onOpenServer={(id) => setSelectedServerId(id)} />
          ) : null}
          {view === 'settings' ? <SettingsPage /> : null}
        </>
      )}
    </AppShell>
  )
}
