# AxolotlServers

A web-based manager for Minecraft servers. No terminal required: create servers, edit configs,
manage players, install add-ons, take scheduled backups and restore them from a dark,
glassmorphic dashboard. An MCP server exposes the same operations and the project's markdown
docs to AI clients.

> **Status: M8.9.1 + backup center delivered; manual review pending.** Server lifecycle,
> telemetry, console, players, file manager, Modrinth App Store (safe Markdown + HTML project
> descriptions), ZIP/path import, ZIP backups with verify/download/retention, recurring tasks and
> the MCP server are implemented. Design: [`docs/architecture.md`](docs/architecture.md).
> Current state: [`docs/state.md`](docs/state.md). Plan: [`docs/todo.md`](docs/todo.md).
> Feature-parity tracker: [`docs/wisehosting_features_roadmap.md`](docs/wisehosting_features_roadmap.md).

## Features

- **19 runtimes** — Vanilla, Paper, Spigot, Purpur, Pufferfish, Folia, Fabric, Forge,
  NeoForge, Quilt, Sponge, Mohist, Arclight, CatServer, Magma, Velocity, BungeeCord,
  Waterfall and Bedrock.
- **Lifecycle** — create, start, stop, restart and delete containers with hard RAM/CPU limits.
- **Create wizard** — runtime category sync, world name/seed/type/difficulty/gamemode
  validation, runtime-specific version discovery for hybrids, and an optional data ZIP imported
  into the new stopped server.
- **App Store** — Modrinth mods, plugins, datapacks, resource packs and modpacks with
  runtime-aware loader filtering for every supported server type.
- **Operations** — live stats/console WebSocket, graphical log parsing, RCON commands,
  player roster and access lists, `server.properties` editor, sandboxed file manager.
- **Backups** — ZIP snapshots with SHA-256 manifests, RCON flush of running servers,
  traversal-safe restore, integrity verification, archive download, keep-last-N retention
  and a control-plane catalog.
- **Scheduled tasks** — recurring backups, start/stop/restart and RCON commands executed by a
  background worker, with last status and error reporting.
- **MCP server (Markdown Context Protocol)** — every repo markdown file as a resource, plus
  typed server/backup/schedule tools under `mcp/`.

## Stack

| Layer          | Choice                                                  |
| -------------- | ------------------------------------------------------- |
| Frontend       | React 19 + Vite 8 + Tailwind CSS 4 (TypeScript)         |
| Control plane  | Rust 2024 · axum · tokio · sqlx (SQLite)                |
| Node agent     | Rust · bollard (Docker Engine API)                      |
| Isolation      | One Docker container per game server, hard limits       |
| Persistence    | SQLite (control plane) + per-server bind mounts         |
| Backups        | ZIP + SHA-256 sidecar manifests on the node             |
| Realtime       | WebSockets (server → client streams); REST for commands |
| AI integration | MCP server (Node 20+, stdio)                            |

## Layout

```
backend/               Rust workspace
  crates/api/          cluster-api    — REST + WS control plane, scheduler
  crates/daemon/       cluster-daemon — Docker, files, RCON, imports, backups
  crates/shared/       cluster-shared — wire types shared by both
frontend/              Vite React app
mcp/                   MCP server (markdown resources + API tools)
docs/                  architecture.md · state.md · todo.md · wisehosting_features_roadmap.md
```

## Quick start

Requires Node 20+, Rust 1.85+ (stable), Docker.

```bash
cp .env.example .env

./start-dev.sh                              # daemon + API + Vite together

# or separately:
cd backend && cargo run -p cluster-api      # http://localhost:8080
cd backend && cargo run -p cluster-daemon   # Docker control channel on :9090
cd frontend && npm install && npm run dev   # http://localhost:5173
```

### MCP server

```bash
cd mcp && npm install
CLUSTER_API_TOKEN=<jwt> npm start           # speaks MCP over stdio
```

See [`mcp/README.md`](mcp/README.md) for client configuration, resources, tools and security
notes.

## Checks

```bash
cd backend && cargo fmt --all --check
cd backend && cargo clippy --workspace --all-targets -- -D warnings
cd backend && cargo test --workspace
cd frontend && npm run build && npm run lint
cd mcp && npm test
```

Or via `make help` for the short list of tasks.

## Working agreement

Before any non-trivial change, read `docs/state.md`. After every milestone, update all three docs
files so they describe reality, not intent.
