import type {
  AddonProjectType,
  AuthResponse,
  ApiErrorBody,
  AuthUser,
  CommandResult,
  CreateServerRequest,
  DirectoryEntry,
  FileContent,
  InstalledAddon,
  PlayerAccessLists,
  PlayerAccessUpdate,
  PlayerAction,
  PlayerHistoryEntry,
  PropertiesResponse,
  ServerView,
  UpdatePropertiesRequest,
  UpdateResourcesRequest,
} from './types'

const TOKEN_KEY = 'cluster.token'

/**
 * Module-level token. Every request goes through `apiFetch`, which reads this
 * value, so components never assemble Authorization headers by hand. This is
 * the equivalent of an Axios request interceptor.
 */
let authToken: string | null = readStoredToken()

function readStoredToken(): string | null {
  try {
    return window.localStorage.getItem(TOKEN_KEY)
  } catch {
    return null
  }
}

export function getToken(): string | null {
  return authToken
}

export function setToken(token: string | null): void {
  authToken = token
  try {
    if (token) window.localStorage.setItem(TOKEN_KEY, token)
    else window.localStorage.removeItem(TOKEN_KEY)
  } catch {
    // Storage can be unavailable (private mode); the in-memory token still works.
  }
}

export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }

  get isPasswordChangeRequired(): boolean {
    return this.code === 'password_change_required'
  }
}

function notify(event: 'cluster:unauthorized' | 'cluster:password-change-required'): void {
  window.dispatchEvent(new CustomEvent(event))
}

async function parseBody(response: Response): Promise<unknown> {
  if (response.status === 204) return undefined
  const text = await response.text()
  if (!text) return undefined
  try {
    return JSON.parse(text) as unknown
  } catch {
    return undefined
  }
}

async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('accept', 'application/json')
  const isFormData =
    typeof FormData !== 'undefined' && init.body instanceof FormData
  if (init.body !== undefined && !isFormData && !headers.has('content-type')) {
    headers.set('content-type', 'application/json')
  }
  if (authToken) headers.set('authorization', `Bearer ${authToken}`)

  let response: Response
  try {
    response = await fetch(path, { ...init, headers })
  } catch {
    throw new ApiError(0, 'network_error', 'Could not reach the control plane.')
  }

  const body = await parseBody(response)

  if (!response.ok) {
    const error = isApiErrorBody(body)
      ? new ApiError(response.status, body.code, body.message)
      : new ApiError(response.status, 'unknown_error', `Request failed (${response.status}).`)

    if (response.status === 401 && !path.startsWith('/api/auth/')) {
      notify('cluster:unauthorized')
    }
    if (error.isPasswordChangeRequired) notify('cluster:password-change-required')
    throw error
  }

  return body as T
}

async function apiFetchBlob(path: string): Promise<Blob> {
  const headers = new Headers()
  if (authToken) headers.set('authorization', `Bearer ${authToken}`)

  let response: Response
  try {
    response = await fetch(path, { headers })
  } catch {
    throw new ApiError(0, 'network_error', 'Could not reach the control plane.')
  }

  if (!response.ok) {
    const body = await parseBody(response)
    const error = isApiErrorBody(body)
      ? new ApiError(response.status, body.code, body.message)
      : new ApiError(response.status, 'unknown_error', `Request failed (${response.status}).`)
    if (response.status === 401) notify('cluster:unauthorized')
    throw error
  }

  return response.blob()
}

function isApiErrorBody(value: unknown): value is ApiErrorBody {
  if (typeof value !== 'object' || value === null) return false
  const record = value as Record<string, unknown>
  return typeof record.code === 'string' && typeof record.message === 'string'
}

export const authApi = {
  login(username: string, password: string): Promise<AuthResponse> {
    return apiFetch<AuthResponse>('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    })
  },

  changePassword(currentPassword: string, newPassword: string): Promise<AuthResponse> {
    return apiFetch<AuthResponse>('/api/auth/change-password', {
      method: 'POST',
      body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
    })
  },

  me(): Promise<AuthUser> {
    return apiFetch<AuthUser>('/api/v1/me')
  },
}

export const serverApi = {
  list(): Promise<ServerView[]> {
    return apiFetch<ServerView[]>('/api/v1/servers')
  },

  get(id: string): Promise<ServerView> {
    return apiFetch<ServerView>(`/api/v1/servers/${encodeURIComponent(id)}`)
  },

  create(request: CreateServerRequest): Promise<ServerView> {
    return apiFetch<ServerView>('/api/v1/servers', {
      method: 'POST',
      body: JSON.stringify(request),
    })
  },

  /** Import server data from a ZIP upload or an absolute host path. */
  importServer(form: FormData): Promise<ServerView> {
    return apiFetch<ServerView>('/api/v1/servers/import', {
      method: 'POST',
      body: form,
    })
  },

  start(id: string): Promise<ServerView> {
    return apiFetch<ServerView>(`/api/v1/servers/${encodeURIComponent(id)}/start`, {
      method: 'POST',
    })
  },

  stop(id: string): Promise<ServerView> {
    return apiFetch<ServerView>(`/api/v1/servers/${encodeURIComponent(id)}/stop`, {
      method: 'POST',
    })
  },

  restart(id: string): Promise<ServerView> {
    return apiFetch<ServerView>(`/api/v1/servers/${encodeURIComponent(id)}/restart`, {
      method: 'POST',
    })
  },

  /** Change the persisted RAM/CPU limits; they apply on the next start/restart. */
  updateResources(id: string, request: UpdateResourcesRequest): Promise<ServerView> {
    return apiFetch<ServerView>(`/api/v1/servers/${encodeURIComponent(id)}/resources`, {
      method: 'PATCH',
      body: JSON.stringify(request),
    })
  },

  /** Online usernames parsed from the server's RCON `list` output. */
  listPlayers(id: string): Promise<string[]> {
    return apiFetch<string[]>(`/api/v1/servers/${encodeURIComponent(id)}/players`)
  },

  /** Players recorded in `usercache.json`; works while the server is stopped. */
  listPlayerHistory(id: string): Promise<PlayerHistoryEntry[]> {
    return apiFetch<PlayerHistoryEntry[]>(
      `/api/v1/servers/${encodeURIComponent(id)}/players/history`,
    )
  },

  /** Operator, whitelist and ban name lists. */
  getPlayerAccess(id: string): Promise<PlayerAccessLists> {
    return apiFetch<PlayerAccessLists>(
      `/api/v1/servers/${encodeURIComponent(id)}/players/access`,
    )
  },

  /** Toggle access for one player, online (RCON) or offline (JSON files). */
  setPlayerAccess(
    id: string,
    username: string,
    update: PlayerAccessUpdate,
  ): Promise<PlayerAccessLists> {
    return apiFetch<PlayerAccessLists>(
      `/api/v1/servers/${encodeURIComponent(id)}/players/${encodeURIComponent(username)}/access`,
      { method: 'PUT', body: JSON.stringify(update) },
    )
  },

  remove(id: string): Promise<void> {
    return apiFetch<void>(`/api/v1/servers/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    })
  },

  /** Installed jar/zip files across the server's add-on directories. */
  listAddons(id: string): Promise<InstalledAddon[]> {
    return apiFetch<InstalledAddon[]>(`/api/v1/servers/${encodeURIComponent(id)}/addons`)
  },

  /** Download a Modrinth version file into the correct server directory. */
  installAddon(
    id: string,
    url: string,
    projectType: AddonProjectType,
  ): Promise<InstalledAddon> {
    return apiFetch<InstalledAddon>(`/api/v1/servers/${encodeURIComponent(id)}/addons/install`, {
      method: 'POST',
      body: JSON.stringify({ url, project_type: projectType }),
    })
  },

  /** Read the parsed, secret-redacted `server.properties`. */
  getProperties(id: string): Promise<PropertiesResponse> {
    return apiFetch<PropertiesResponse>(`/api/v1/servers/${encodeURIComponent(id)}/properties`)
  },

  /** Atomically write `server.properties`; `expected_hash` detects conflicts. */
  updateProperties(
    id: string,
    entries: UpdatePropertiesRequest['entries'],
    expectedHash?: string,
  ): Promise<PropertiesResponse> {
    return apiFetch<PropertiesResponse>(`/api/v1/servers/${encodeURIComponent(id)}/properties`, {
      method: 'PUT',
      body: JSON.stringify({ entries, expected_hash: expectedHash }),
    })
  },

  /** List the immediate children of a server data directory (`''` = root). */
  listDirectory(id: string, path = ''): Promise<DirectoryEntry[]> {
    const suffix = path ? `/${encodePath(path)}` : ''
    return apiFetch<DirectoryEntry[]>(
      `/api/v1/servers/${encodeURIComponent(id)}/directory${suffix}`,
    )
  },

  /** Read a UTF-8 text file from a server's data volume. */
  readFile(id: string, path: string): Promise<FileContent> {
    return apiFetch<FileContent>(
      `/api/v1/servers/${encodeURIComponent(id)}/files/${encodePath(path)}`,
    )
  },

  /** Atomically replace a text file; `expectedHash` detects concurrent edits. */
  writeFile(
    id: string,
    path: string,
    content: string,
    expectedHash?: string,
  ): Promise<FileContent> {
    return apiFetch<FileContent>(
      `/api/v1/servers/${encodeURIComponent(id)}/files/${encodePath(path)}`,
      {
        method: 'PUT',
        body: JSON.stringify({ content, expected_hash: expectedHash }),
      },
    )
  },

  /** Delete a regular file from a server's data volume. */
  deleteFile(id: string, path: string): Promise<void> {
    return apiFetch<void>(
      `/api/v1/servers/${encodeURIComponent(id)}/files/${encodePath(path)}`,
      { method: 'DELETE' },
    )
  },

  /** Create an empty directory inside a server's data volume. */
  createDirectory(id: string, path: string): Promise<void> {
    return apiFetch<void>(
      `/api/v1/servers/${encodeURIComponent(id)}/directory/${encodePath(path)}`,
      { method: 'POST' },
    )
  },

  /** Rename or move a file/directory within the server data volume. */
  renameFile(id: string, oldPath: string, newPath: string): Promise<void> {
    return apiFetch<void>(`/api/v1/servers/${encodeURIComponent(id)}/files/rename`, {
      method: 'POST',
      body: JSON.stringify({ old_path: oldPath, new_path: newPath }),
    })
  },

  /** Copy a file or directory within the server data volume. */
  copyFile(id: string, sourcePath: string, destPath: string): Promise<void> {
    return apiFetch<void>(`/api/v1/servers/${encodeURIComponent(id)}/files/copy`, {
      method: 'POST',
      body: JSON.stringify({ source_path: sourcePath, dest_path: destPath }),
    })
  },

  /** Upload a binary file, replacing any existing file at the path. */
  uploadFile(id: string, path: string, data: ArrayBuffer): Promise<void> {
    return apiFetch<void>(
      `/api/v1/servers/${encodeURIComponent(id)}/files/${encodePath(path)}`,
      {
        method: 'POST',
        headers: { 'content-type': 'application/octet-stream' },
        body: data,
      },
    )
  },

  /** Download a file's raw bytes (works for binaries as well as text). */
  downloadFile(id: string, path: string): Promise<Blob> {
    return apiFetchBlob(
      `/api/v1/servers/${encodeURIComponent(id)}/files/${encodePath(path)}?download=true`,
    )
  },

  /** Execute a raw Minecraft command (leading slash optional). */
  command(id: string, command: string): Promise<CommandResult> {
    return apiFetch<CommandResult>(`/api/v1/servers/${encodeURIComponent(id)}/command`, {
      method: 'POST',
      body: JSON.stringify({ command }),
    })
  },

  /** Kick, ban or op a player through the semantic RCON routes. */
  playerAction(
    id: string,
    username: string,
    action: PlayerAction,
    reason?: string,
  ): Promise<CommandResult> {
    const path = `/api/v1/servers/${encodeURIComponent(id)}/players/${encodeURIComponent(username)}/${action}`
    if (action === 'op') {
      return apiFetch<CommandResult>(path, { method: 'POST' })
    }
    return apiFetch<CommandResult>(path, {
      method: 'POST',
      body: JSON.stringify({ reason: reason?.trim() || undefined }),
    })
  },
}

/** Percent-encode each segment but keep the `/` separators. */
function encodePath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/')
}

/**
 * WebSocket URL for a server's telemetry. The token is passed as a query
 * parameter because browsers cannot attach an Authorization header to the
 * upgrade request; the Rust API also accepts a `bearer.<jwt>` subprotocol.
 */
export function telemetrySocketUrl(serverId: string): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const url = new URL(
    `${protocol}//${window.location.host}/api/v1/ws/servers/${encodeURIComponent(serverId)}`,
  )
  const token = getToken()
  if (token) url.searchParams.set('token', token)
  return url.toString()
}
