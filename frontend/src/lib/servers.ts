import type { ServerStatus, ServerType } from './types'

export interface ServerTypeMeta {
  value: ServerType
  label: string
  tagline: string
  description: string
  accent: string
  glow: string
  monogram: string
  proxy: boolean
  category: 'Official' | 'Plugins' | 'Modded' | 'Hybrid' | 'Proxy' | 'Bedrock'
}

export const SERVER_TYPES: readonly ServerTypeMeta[] = [
  {
    value: 'vanilla',
    label: 'Vanilla',
    tagline: 'Pure Mojang',
    description: 'The reference survival experience. Maximum compatibility, zero plugins.',
    accent: 'text-neon-emerald',
    glow: 'shadow-glow-emerald',
    monogram: 'V',
    proxy: false,
    category: 'Official',
  },
  {
    value: 'paper',
    label: 'Paper',
    tagline: 'Optimised',
    description: 'High-performance Spigot fork with plugin support and configurable patches.',
    accent: 'text-neon-cyan',
    glow: 'shadow-glow-cyan',
    monogram: 'P',
    proxy: false,
    category: 'Plugins',
  },
  {
    value: 'fabric',
    label: 'Fabric',
    tagline: 'Modded',
    description: 'Lightweight mod loader for modern versions and client-side friendly modpacks.',
    accent: 'text-neon-violet',
    glow: 'shadow-glow-violet',
    monogram: 'F',
    proxy: false,
    category: 'Modded',
  },
  {
    value: 'forge',
    label: 'Forge',
    tagline: 'Modded',
    description: 'The long-standing mod loader. Broad modpack support with a large mod ecosystem.',
    accent: 'text-neon-amber',
    glow: 'shadow-glow-amber',
    monogram: 'Fo',
    proxy: false,
    category: 'Modded',
  },
  {
    value: 'velocity',
    label: 'Velocity',
    tagline: 'Proxy',
    description: 'Modern, fast proxy. Owns the public port and routes players to backends.',
    accent: 'text-neon-amber',
    glow: 'shadow-glow-amber',
    monogram: 'V',
    proxy: true,
    category: 'Proxy',
  },
  {
    value: 'bungeecord',
    label: 'BungeeCord',
    tagline: 'Proxy',
    description: 'Battle-tested proxy for legacy networks and BungeeCord forwarding.',
    accent: 'text-neon-rose',
    glow: 'shadow-glow-rose',
    monogram: 'B',
    proxy: true,
    category: 'Proxy',
  },
  ...[
    ['bedrock','Bedrock','Bedrock Edition','Bedrock dedicated server','B','Bedrock'],
    ['neoforge','NeoForge','Modded','Modern Forge successor','N','Modded'],
    ['quilt','Quilt','Modded','Fabric-compatible mod loader','Q','Modded'],
    ['spigot','Spigot','Plugins','Classic Bukkit plugin server','S','Plugins'],
    ['purpur','Purpur','Plugins','Highly configurable Paper fork','Pu','Plugins'],
    ['pufferfish','Pufferfish','Plugins','Performance-focused Paper fork','Pf','Plugins'],
    ['folia','Folia','Plugins','Region-threaded Paper fork','F','Plugins'],
    ['sponge','Sponge','Plugins','SpongeVanilla plugin platform','Sp','Plugins'],
    ['mohist','Mohist','Hybrid','Forge mods and Bukkit plugins','M','Hybrid'],
    ['arclight','Arclight','Hybrid','Mod and plugin hybrid server','A','Hybrid'],
    ['catserver','CatServer','Hybrid','Forge and Bukkit hybrid','C','Hybrid'],
    ['magma','Magma','Hybrid','Forge and Bukkit hybrid','Mg','Hybrid'],
    ['waterfall','Waterfall','Proxy','BungeeCord proxy fork','W','Proxy'],
  ].map(([value, label, tagline, description, monogram, category]) => ({
    value: value as ServerType, label, tagline, description, monogram, category: category as ServerTypeMeta['category'],
    accent: 'text-neon-violet', glow: 'shadow-glow-violet', proxy: category === 'Proxy',
  })),
] as const

export function serverTypeMeta(type: ServerType): ServerTypeMeta {
  return SERVER_TYPES.find((meta) => meta.value === type) ?? SERVER_TYPES[0]
}

export interface StatusMeta {
  label: string
  dot: string
  text: string
  chip: string
}

export const STATUS_META: Record<ServerStatus, StatusMeta> = {
  running: {
    label: 'Running',
    dot: 'bg-neon-emerald',
    text: 'text-neon-emerald',
    chip: 'border-neon-emerald/30 bg-neon-emerald/10',
  },
  stopped: {
    label: 'Stopped',
    dot: 'bg-text-dim',
    text: 'text-text-muted',
    chip: 'border-white/10 bg-white/5',
  },
  provisioning: {
    label: 'Provisioning',
    dot: 'bg-neon-amber',
    text: 'text-neon-amber',
    chip: 'border-neon-amber/30 bg-neon-amber/10',
  },
  starting: {
    label: 'Starting',
    dot: 'bg-neon-cyan',
    text: 'text-neon-cyan',
    chip: 'border-neon-cyan/30 bg-neon-cyan/10',
  },
  stopping: {
    label: 'Stopping',
    dot: 'bg-neon-cyan',
    text: 'text-neon-cyan',
    chip: 'border-neon-cyan/30 bg-neon-cyan/10',
  },
  error: {
    label: 'Error',
    dot: 'bg-neon-rose',
    text: 'text-neon-rose',
    chip: 'border-neon-rose/30 bg-neon-rose/10',
  },
}

export function formatRam(ramMb: number): string {
  const gb = ramMb / 1024
  return `${Number.isInteger(gb) ? gb : gb.toFixed(1)} GB`
}

export function formatCpu(cpu: number): string {
  return `${Number.isInteger(cpu) ? cpu : cpu.toFixed(1)} vCPU`
}

export function formatDate(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return '—'
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}
