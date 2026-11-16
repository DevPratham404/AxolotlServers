/** Wire types mirrored from `cluster-shared` and the control-plane DTOs. */

export type ServerType = 'vanilla' | 'paper' | 'fabric' | 'forge' | 'velocity' | 'bungeecord' | 'bedrock' | 'neoforge' | 'quilt' | 'spigot' | 'purpur' | 'pufferfish' | 'folia' | 'sponge' | 'mohist' | 'arclight' | 'catserver' | 'magma' | 'waterfall'

export type ServerStatus =
  | 'provisioning'
  | 'stopped'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'error'

export interface AuthUser {
  id: string
  username: string
  requires_password_change: boolean
}

export interface AuthResponse {
  token: string
  token_type: 'Bearer'
  expires_at: number
  user: AuthUser
}

export interface ServerView {
  id: string
  name: string
  game_port: number
  server_type: ServerType
  game_version: string
  ram_mb: number
  cpu_limit: number
  is_behind_proxy: boolean
  proxy_network_id: string | null
  status: ServerStatus
  created_at: string
  updated_at: string
  world_name: string
  world_seed: string
  world_type: string
  difficulty: string
  gamemode: string
}

export interface CreateServerRequest {
  name: string
  server_type: ServerType
  game_version: string
  game_port: number
  ram_mb: number
  cpu_limit: number
  is_behind_proxy?: boolean
  proxy_network_id?: string | null
  world_name?: string
  world_seed?: string
  world_type?: string
  difficulty?: string
  gamemode?: string
}

export interface ApiErrorBody {
  code: string
  message: string
}

/** Telemetry envelopes sent over `/api/v1/ws/servers/{id}`. */

export interface StatsSample {
  cpu_percent: number
  memory_used_bytes: number
  memory_limit_bytes: number
  memory_percent: number
  network_rx_bytes: number
  network_tx_bytes: number
  pids: number
  timestamp_ms: number
}

export type LogStreamKind = 'stdout' | 'stderr'

export interface LogLine {
  stream: LogStreamKind
  timestamp_ms: number
  message: string
}

export type TelemetryMessage =
  | { type: 'ready'; server_id: string }
  | { type: 'stats'; sample: StatsSample }
  | { type: 'log'; line: LogLine }
  | { type: 'stream_ended'; stream: string }
  | { type: 'error'; code: string; message: string }

export type TelemetryStatus = 'connecting' | 'live' | 'ended' | 'error'

/** Result of a raw or semantic command executed through RCON. */
export interface CommandResult {
  command: string
  output: string
}

/**
 * One player in the combined roster. Online players come from the live RCON
 * `list`; offline players come from `usercache.json`. Richer live fields remain
 * unknown until the companion plugin lands.
 */
export interface PlayerSummary {
  username: string
  online: boolean
  uuid: string | null
  operator: boolean
  whitelisted: boolean
  banned: boolean
  ping: number | null
  gamemode?: 'Survival' | 'Creative' | 'Adventure' | 'Spectator'
  joinedAgo?: string
}

export type PlayerAction = 'kick' | 'ban' | 'op'

/** One `usercache.json` entry — a player who has joined this server before. */
export interface PlayerHistoryEntry {
  name: string
  uuid: string
  expires_on: string | null
}

/** Name lists read from ops.json, whitelist.json and banned-players.json. */
export interface PlayerAccessLists {
  operators: string[]
  whitelisted: string[]
  banned: string[]
}

/** Partial access toggle; omitted fields remain unchanged. */
export interface PlayerAccessUpdate {
  operator?: boolean
  whitelisted?: boolean
  banned?: boolean
}

/** Modrinth project kinds supported by the App Store. */
export type AddonProjectType = 'mod' | 'plugin' | 'datapack' | 'resourcepack' | 'modpack'

/** Request body for `POST /servers/{id}/addons/install`. */
export interface AddonInstallRequest {
  /** Direct Modrinth `cdn.modrinth.com` version file URL. */
  url: string
  project_type: AddonProjectType
}

/** One installed jar/zip discovered in a dynamic add-on directory. */
export interface InstalledAddon {
  name: string
  /** Volume-relative path, e.g. `plugins/Example.jar`. */
  path: string
  directory: string
  size: number
  project_type: AddonProjectType
}

export interface UpdateResourcesRequest {
  ram_mb?: number
  cpu_limit?: number
}

/** A ZIP snapshot stored on the daemon host. */
export interface BackupInfo {
  id: string
  name: string
  size_bytes: number
  sha256: string
  created_at_ms: number
  server_was_running: boolean
}

/** Result of re-hashing a stored archive against its manifest. */
export interface BackupVerification {
  id: string
  verified: boolean
  expected_sha256: string
  actual_sha256: string
}

/** Retention policy for a single server; `0` keeps every backup. */
export interface BackupSettings {
  server_id: string
  retention_count: number
}

export type ScheduleTaskType = 'backup' | 'start' | 'stop' | 'restart' | 'command'

/** Recurring automation owned by the control plane. */
export interface ScheduleView {
  id: string
  server_id: string
  name: string
  task_type: ScheduleTaskType
  command: string | null
  interval_minutes: number
  enabled: boolean
  next_run_at: string
  last_run_at: string | null
  last_status: 'success' | 'error' | null
  last_error: string | null
  created_at: string
  updated_at: string
}

export interface CreateScheduleRequest {
  name: string
  task_type: ScheduleTaskType
  command?: string
  interval_minutes: number
  enabled?: boolean
  run_now?: boolean
}

export interface UpdateScheduleRequest {
  name?: string
  command?: string
  interval_minutes?: number
  enabled?: boolean
}

/** One logical line of `server.properties`, mirrored from the API parser. */
export type PropertyEntry =
  | { kind: 'comment'; text: string }
  | { kind: 'blank' }
  | { kind: 'property'; key: string; value: string }

export interface PropertiesResponse {
  path: string
  hash: string
  entries: PropertyEntry[]
  /** Keys withheld from the browser; values are restored on save. */
  redacted: string[]
}

export interface UpdatePropertiesRequest {
  entries: PropertyEntry[]
  expected_hash?: string
}

/** One child in a server's data volume. */
export interface DirectoryEntry {
  name: string
  /** Bytes; `0` for directories. */
  size: number
  is_dir: boolean
}

/** Raw text file as returned by the daemon through the control plane. */
export interface FileContent {
  path: string
  content: string
  hash: string
}
