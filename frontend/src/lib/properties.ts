/** Schema for the visual `server.properties` editor. */

export type FieldKind = 'boolean' | 'enum' | 'number' | 'string' | 'secret'

export interface SelectOption {
  value: string
  label: string
}

export interface PropertyField {
  key: string
  label: string
  kind: FieldKind
  section: SectionId
  hint?: string
  options?: SelectOption[]
  min?: number
  max?: number
  step?: number
  /** Render as a neon range slider instead of a boxed number input. */
  slider?: boolean
  unit?: string
}

export type SectionId = 'identity' | 'gameplay' | 'world' | 'network' | 'security' | 'performance'

export interface ConfigSection {
  id: SectionId
  title: string
  description: string
  accent: string
  glow: string
}

export const SECTIONS: ConfigSection[] = [
  {
    id: 'identity',
    title: 'Identity',
    description: 'How the server presents itself in the multiplayer list.',
    accent: 'text-neon-cyan',
    glow: 'shadow-glow-cyan',
  },
  {
    id: 'gameplay',
    title: 'Gameplay',
    description: 'Rules that shape moment-to-moment play.',
    accent: 'text-neon-emerald',
    glow: 'shadow-glow-emerald',
  },
  {
    id: 'world',
    title: 'World',
    description: 'Generation, distance and protection settings.',
    accent: 'text-neon-violet',
    glow: 'shadow-glow-violet',
  },
  {
    id: 'network',
    title: 'Network',
    description: 'Connections, authentication and player limits.',
    accent: 'text-neon-cyan',
    glow: 'shadow-glow-cyan',
  },
  {
    id: 'security',
    title: 'Security & RCON',
    description: 'Remote console access and server hardening.',
    accent: 'text-neon-rose',
    glow: 'shadow-glow-rose',
  },
  {
    id: 'performance',
    title: 'Performance',
    description: 'Ticks, chunks and transport tuning.',
    accent: 'text-neon-amber',
    glow: 'shadow-glow-amber',
  },
]

const GAMEMODES: SelectOption[] = [
  { value: 'survival', label: 'Survival' },
  { value: 'creative', label: 'Creative' },
  { value: 'adventure', label: 'Adventure' },
  { value: 'spectator', label: 'Spectator' },
]

const DIFFICULTIES: SelectOption[] = [
  { value: 'peaceful', label: 'Peaceful' },
  { value: 'easy', label: 'Easy' },
  { value: 'normal', label: 'Normal' },
  { value: 'hard', label: 'Hard' },
]

const LEVEL_TYPES: SelectOption[] = [
  { value: 'minecraft:normal', label: 'Normal' },
  { value: 'minecraft:flat', label: 'Superflat' },
  { value: 'minecraft:large_biomes', label: 'Large biomes' },
  { value: 'minecraft:amplified', label: 'Amplified' },
  { value: 'minecraft:single_biome_surface', label: 'Single biome' },
]

const PERMISSION_LEVELS: SelectOption[] = [
  { value: '1', label: '1 — bypass spawn protection' },
  { value: '2', label: '2 — operator commands' },
  { value: '3', label: '3 — player management' },
  { value: '4', label: '4 — full control' },
]

export const FIELDS: PropertyField[] = [
  // Identity
  {
    key: 'motd',
    label: 'Message of the day',
    kind: 'string',
    section: 'identity',
    hint: 'Supports legacy colour codes such as \u00a7a.',
  },
  { key: 'level-name', label: 'World folder', kind: 'string', section: 'identity' },
  { key: 'level-seed', label: 'World seed', kind: 'string', section: 'identity' },
  {
    key: 'server-port',
    label: 'Server port',
    kind: 'number',
    section: 'identity',
    min: 1,
    max: 65535,
  },

  // Gameplay
  { key: 'gamemode', label: 'Default gamemode', kind: 'enum', section: 'gameplay', options: GAMEMODES },
  {
    key: 'difficulty',
    label: 'Difficulty',
    kind: 'enum',
    section: 'gameplay',
    options: DIFFICULTIES,
  },
  { key: 'hardcore', label: 'Hardcore mode', kind: 'boolean', section: 'gameplay' },
  { key: 'pvp', label: 'Player versus player', kind: 'boolean', section: 'gameplay' },
  { key: 'force-gamemode', label: 'Force gamemode on join', kind: 'boolean', section: 'gameplay' },
  { key: 'allow-flight', label: 'Allow flight', kind: 'boolean', section: 'gameplay' },
  { key: 'spawn-monsters', label: 'Spawn monsters', kind: 'boolean', section: 'gameplay' },
  { key: 'generate-structures', label: 'Generate structures', kind: 'boolean', section: 'gameplay' },

  // World
  {
    key: 'level-type',
    label: 'World type',
    kind: 'enum',
    section: 'world',
    options: LEVEL_TYPES,
  },
  { key: 'allow-nether', label: 'Allow the Nether', kind: 'boolean', section: 'world' },
  {
    key: 'view-distance',
    label: 'View distance',
    kind: 'number',
    section: 'world',
    min: 2,
    max: 32,
    slider: true,
    unit: 'chunks',
  },
  {
    key: 'simulation-distance',
    label: 'Simulation distance',
    kind: 'number',
    section: 'world',
    min: 3,
    max: 32,
    slider: true,
    unit: 'chunks',
  },
  {
    key: 'spawn-protection',
    label: 'Spawn protection',
    kind: 'number',
    section: 'world',
    min: 0,
    max: 1000,
    slider: true,
    unit: 'blocks',
  },
  {
    key: 'entity-broadcast-range-percentage',
    label: 'Entity broadcast range',
    kind: 'number',
    section: 'world',
    min: 10,
    max: 1000,
    slider: true,
    unit: '%',
  },
  {
    key: 'max-world-size',
    label: 'Maximum world size',
    kind: 'number',
    section: 'world',
    min: 1,
    max: 29_999_984,
    unit: 'blocks',
  },

  // Network
  { key: 'online-mode', label: 'Online mode', kind: 'boolean', section: 'network', hint: 'Verify players with Mojang sessions.' },
  { key: 'white-list', label: 'Whitelist', kind: 'boolean', section: 'network' },
  { key: 'enforce-whitelist', label: 'Enforce whitelist', kind: 'boolean', section: 'network' },
  {
    key: 'max-players',
    label: 'Maximum players',
    kind: 'number',
    section: 'network',
    min: 1,
    max: 500,
  },
  { key: 'enable-command-block', label: 'Command blocks', kind: 'boolean', section: 'network' },
  { key: 'enable-status', label: 'Advertise status', kind: 'boolean', section: 'network' },
  { key: 'hide-online-players', label: 'Hide online players', kind: 'boolean', section: 'network' },
  { key: 'prevent-proxy-connections', label: 'Block proxy connections', kind: 'boolean', section: 'network' },
  {
    key: 'network-compression-threshold',
    label: 'Compression threshold',
    kind: 'number',
    section: 'network',
    min: -1,
    max: 65_535,
    unit: 'bytes',
  },
  {
    key: 'player-idle-timeout',
    label: 'Idle timeout',
    kind: 'number',
    section: 'network',
    min: 0,
    max: 1440,
    unit: 'minutes',
  },
  {
    key: 'op-permission-level',
    label: 'Operator permission level',
    kind: 'enum',
    section: 'network',
    options: PERMISSION_LEVELS,
  },

  // Security & RCON
  { key: 'enable-rcon', label: 'Enable RCON', kind: 'boolean', section: 'security' },
  { key: 'rcon.port', label: 'RCON port', kind: 'number', section: 'security', min: 1, max: 65535 },
  {
    key: 'rcon.password',
    label: 'RCON password',
    kind: 'secret',
    section: 'security',
    hint: 'Managed by Cluster Console and never exposed to the browser.',
  },
  { key: 'broadcast-rcon-to-ops', label: 'Broadcast RCON output', kind: 'boolean', section: 'security' },
  { key: 'enable-query', label: 'Enable query protocol', kind: 'boolean', section: 'security' },
  { key: 'query.port', label: 'Query port', kind: 'number', section: 'security', min: 1, max: 65535 },

  // Performance
  { key: 'sync-chunk-writes', label: 'Synchronous chunk writes', kind: 'boolean', section: 'performance' },
  { key: 'use-native-transport', label: 'Native transport', kind: 'boolean', section: 'performance' },
  { key: 'max-tick-time', label: 'Maximum tick time', kind: 'number', section: 'performance', min: -1, unit: 'ms' },
  { key: 'rate-limit', label: 'Packet rate limit', kind: 'number', section: 'performance', min: 0, unit: 'packets/s' },
  { key: 'max-chained-neighbor-updates', label: 'Chained neighbour updates', kind: 'number', section: 'performance', min: -1 },
  {
    key: 'pause-when-empty-seconds',
    label: 'Pause when empty',
    kind: 'number',
    section: 'performance',
    min: -1,
    unit: 'seconds',
    hint: 'Use -1 to disable automatic pausing.',
  },
]

export const FIELDS_BY_SECTION = SECTIONS.map((section) => ({
  section,
  fields: FIELDS.filter((field) => field.section === section.id),
}))

export const FIELD_BY_KEY = new Map(FIELDS.map((field) => [field.key, field]))

export function fieldsForSection(section: SectionId): PropertyField[] {
  return FIELDS.filter((field) => field.section === section)
}

/** Clamp/normalise a raw value for a numeric field, returning null if unusable. */
export function parseNumber(field: PropertyField, raw: string): number | null {
  const trimmed = raw.trim()
  if (trimmed === '') return null
  const value = Number(trimmed)
  if (!Number.isFinite(value)) return null
  if (field.min !== undefined && value < field.min) return null
  if (field.max !== undefined && value > field.max) return null
  return value
}
