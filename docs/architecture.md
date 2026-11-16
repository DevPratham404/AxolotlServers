# Architecture — Cluster Console

> Status: **M8.8 + automation delivered; manual review pending.** Backend is Rust; the glass
> frontend, auth flow, server lifecycle, telemetry WebSocket, graphical console, player tools, file
> manager, Modrinth App Store, ZIP/local-path server importer, ZIP backups, recurring tasks and the
> MCP server are implemented. Oracle VPS Nginx and systemd files are in `deployment/`. Decisions
> D1–D4, D9–D14 are accepted; open items are listed in §13. Complete the manual review before
> proceeding to M10 (multi-node).

## 1. Goal & scope

One web dashboard that operates Minecraft servers — create, start, stop, configure, inspect
players, back up and restore — without the user ever opening a shell.

**In scope (v1, single node)**

- Server lifecycle: create from a wizard, start/stop/restart/kill, delete.
- Per-server hard RAM/CPU limits enforced by Docker.
- Visual `server.properties` + whitelist/ops/bans editors.
- Live player roster with stat/inventory/permission editing (assisted by a companion plugin).
- Backup center (create, verify, download, restore, delete, keep-last-N retention).
- **Optional BungeeCord / Velocity / Waterfall proxies**: a proxy is a first-class managed container
  (`server_type = velocity | bungeecord | waterfall`) that routes players to backend game servers
  grouped in a proxy network.
- Local accounts with JWT sessions.
- Recurring backups/power/RCON tasks executed by an in-process scheduler.
- Modrinth App Store for mods, plugins, datapacks, resource packs and modpacks.
- An MCP server exposing markdown documentation and the REST API to AI clients.

**Out of scope (v1, revisit later)**

- CurseForge integration and a paid plugin marketplace.
- Multi-tenant billing, teams, SSO/OIDC.
- Automatic horizontal scaling; recurring tasks are single-node only.
- Authoritative live vitals/inventory (companion plugin).
- Backup upload, locked snapshots, off-host sync and notifications.
- Multi-node operation — **schema is ready for it**, the UI/flow is not built yet.

## 2. Design principles

1. **The UI never guesses.** Every mutation is a typed API call with a validated payload; the
   frontend renders server-reported state, not optimistic fiction (optimistic UI allowed only for
   power transitions, reconciled by the next event).
2. **The control plane never holds the Docker socket.** Only the node daemon does. A compromised web
   process cannot escape to the host.
3. **One container per server.** No shared JVM, no shared world directory, no ambient authority.
   Limits are enforced by the container runtime, not by conventions.
4. **Idempotent commands.** Power actions carry a client-generated request ID; the daemon
   de-duplicates retries. Long operations are `jobs` with progress events.
5. **Files are the contract.** Minecraft owns its files. We edit them atomically, validate before
   writing, and never rewrite a file we did not parse.
6. **Types at the boundary.** Shared wire types live in one crate (`cluster-shared`); Rust's type
   system replaces a separate schema-validation layer where possible.
7. **The docs are the memory.** Any change that alters these decisions updates this file and
   `state.md` in the same session.

## 3. Topology

```
                    ┌──────────────────────────────┐
   Browser ──HTTPS──▶  Caddy / reverse proxy       │
             WSS     │  (TLS, static SPA, /api,/ws) │
                    └──────────────┬───────────────┘
                                   │
                    ┌──────────────▼───────────────┐
                    │  Control plane  (cluster-api)│
                    │  Rust · axum · tokio         │
                    │  REST + WS hub               │
                    │  auth, RBAC, jobs, audit     │
                    │  SQLite via sqlx             │
                    └───────┬──────────────────────┘
                            │ HTTP (HMAC-signed, TLS in prod)
                    ┌───────▼──────────────────────┐
                    │  Node daemon (cluster-daemon)│
                    │  Rust · axum · bollard       │
                    │  Docker Engine API           │
                    │  ┌────────────────────────┐  │
                    │  │ mc-<serverId> container│  │
                    │  │ itzg/minecraft-server  │  │
                    │  │ 4 GB / 2 CPU (limited) │  │
                    │  └────────────────────────┘  │
                    └──────────────────────────────┘
```

v1 runs both processes on one VPS. The daemon/control-plane split exists from day one so adding
nodes later is a registration, not a rewrite. `nodes` exists in the schema from the first
migration and every server carries `node_id`.

## 4. Components

### 4.1 Frontend — `frontend/`

- React 19 + TypeScript + Vite 8, Tailwind CSS 4 (`@tailwindcss/vite`, no PostCSS config).
- State: a single `AuthProvider` owns the session; page state is ordinary React state. Server data
  is fetched through a typed `apiFetch` wrapper (`lib/api.ts`) that attaches the bearer token
  centrally and converts non-2xx bodies into `ApiError`. Live resources/logs are owned by
  `useServerTelemetry`, which keeps one WebSocket per open server detail view. There is no global
  store or query cache yet.
- Routing: no router dependency yet — `App` switches between login, forced rotation, the three
  shell views (`dashboard`, `servers`, `settings`) and a selected server detail page. URL routing
  lands with deep links.
- Visual language: near-black base (`#05060a`), frosted panels (`backdrop-filter: blur(22px)`),
  neon emerald/cyan/violet/amber/rose accents, display font Outfit + JetBrains Mono for
  console/values. Tokens live in `src/index.css` under `@theme`; the aurora backdrop is a local SVG
  (`public/aurora-bg.svg`) behind a `bg-black/50 backdrop-blur-xl` overlay.
- Dev proxy: Vite forwards `/api` **including WebSocket upgrades** (`ws: true`) to
  `localhost:8080`, so the SPA only uses same-origin URLs.

Implemented structure:

```
src/
  App.tsx                     auth gate + view switch + selected server
  auth/                       AuthProvider, context/hook, splash
  components/
    layout/                   AppShell, Sidebar, AuroraBackdrop
    ui/                       Button, GlassCard, StatusPill, TextField, Feedback, PageHeader
  features/
    auth/                     LoginPage, ChangePasswordPage
    dashboard/                DashboardPage (stats, recent servers, posture)
    servers/                  ServersPage, ServerCard, CreateServerWizard,
                              ImportServerWizard, useServers
    server-detail/            ServerDetailPage, StatsPanel, LogConsole, ConfigurationPanel,
                              AppStorePanel, AutomationPanel, FileManagerPanel, players/*,
                              telemetry hook
    settings/                 SettingsPage
  lib/                        api + WS URL, JWT decode, wire types, server metadata, log parser,
                              properties schema
```

Session approach: the JWT is persisted in `localStorage` (`cluster.token`), decoded for the first
paint, then confirmed with `GET /api/v1/me`. A DOM event (`cluster:unauthorized`,
`cluster:password-change-required`) lets the API layer drive auth screens without coupling pages to
the provider. `401` on non-auth routes signs the user out; the `403` password gate switches to the
rotation screen. All privileged decisions remain server-side.

Telemetry approach: `useServerTelemetry` opens
`ws(s)://<origin>/api/v1/ws/servers/{id}?token=<jwt>` while the server is running, reconnects with
capped exponential backoff, keeps a 600-line ring buffer, and never stores telemetry in global
state. `lib/logs.ts` parses Minecraft/Paper/proxy lines into time, source, level and a visual tone
(`info`, `warn`, `error`, `join`, `leave`, `chat`, `debug`, `system`) for the graphical console.

Player UX (M5-Part 1): the detail page has `Console`, `Players`, `Configuration`, `File Manager`,
`App Store` and `Automation` tabs. The roster uses preview data, Crafatar avatars with an initials
fallback, ping bars and hover actions wired to the real RCON routes. The slide-out inspection drawer
renders disabled Vitals sliders and a 9×4 inventory grid behind a "Requires Companion Plugin" lock;
no player state is read from the server yet.

App Store UX (M8): `AppStorePanel` maps Modrinth project types to runtime folders. Capability
helpers cover all 19 runtimes: mods/modpacks for Fabric/Forge/NeoForge/Quilt and hybrid servers,
plugins for Bukkit-family runtimes, Sponge and proxies, datapacks/resource packs everywhere.
Loader filters are derived from the same helpers so the search UI cannot offer an incompatible
project type. The details modal renders Modrinth bodies with `react-markdown` + `remark-gfm`, then
runs `rehype-raw` + `rehype-sanitize` so project-authored HTML (`<center>`, images, `<br>`,
details/summary) renders instead of leaking as text. Iframes are mounted only for an allow-list of
HTTPS YouTube/Vimeo hosts and receive a sandbox; non-HTTPS `src` values are stripped.

Automation UX: `AutomationPanel` lists ZIP backups and scheduled tasks, creates backups with an
optional label, verifies them against their SHA-256 manifest, downloads archives, configures
keep-last-N retention, restores with an explicit confirmation, and creates/pauses/runs/deletes
backup, power and RCON-command schedules. Destructive actions refresh the parent page so power
state changes are reflected immediately.

Create wizard (M3/M8.8): runtime categories and the selected card stay in sync; world fields are
validated client- and server-side. An optional data ZIP is uploaded **after** the container is
created (stopped) through `POST /api/v1/servers/{id}/import-data`; if the import fails the wizard
keeps the created server and offers "Open server" instead of silently dropping the archive.

Configuration UX (M6): `lib/properties.ts` holds a curated schema (sections, labels, enum options,
number ranges, sliders) mapping properties to glassmorphic toggles, selects, range/number inputs and
text fields. `ConfigurationPanel` loads the parsed entry list, keeps comments/blanks untouched in
state, marks itself dirty on edit, saves the whole document with the last hash, and shows a restart
modal on success. Keys outside the schema render in a "Custom properties" card; redacted secrets
render as a locked placeholder.

### 4.2 Control plane — `backend/crates/api` (`cluster-api`)

Responsibilities: identity, authorization, node registry, port allocation, desired-state store,
job orchestration, WS fan-out to browsers, audit log, backup catalog. It never reads server files or
talks to Docker directly; file access is delegated to the daemon over the signed channel.

Cargo dependencies (workspace-pinned): `axum 0.8` (ws), `tokio`, `tower`/`tower-http`, `sqlx 0.9`
(SQLite, bundled, migrations), `jsonwebtoken 11`, `argon2 0.6`, `serde`, `uuid` (v7), `time`,
`tracing`/`tracing-subscriber`, `thiserror` (+ `anyhow` at the binary edge).

| Module                | Status | Responsibility                                                     |
| --------------------- | ------ | ------------------------------------------------------------------ |
| `routes/`             | ✅ M2  | `/healthz`, `/readyz`, `/api/auth/*`, `/api/v1/*`                   |
| `routes/auth.rs`      | ✅ M2  | `POST /api/auth/login`, `POST /api/auth/change-password`            |
| `routes/servers.rs`   | ✅ M4  | server CRUD, start/stop/restart, create validation, `import-data` |
| `routes/addons.rs`    | ✅ M8  | Modrinth install routing for every runtime capability             |
| `routes/files.rs`     | ✅ M7  | file manager browsing, upload/download, rename/copy/delete        |
| `routes/automation.rs`| ✅ new | backup CRUD/restore/download/verify/retention + schedule CRUD/run |
| `scheduler.rs`        | ✅ new | 20 s worker for recurring tasks and backup cataloging            |
| `routes/players.rs`   | ✅ M5  | raw `/command` + semantic `players/{username}/kick|ban|op`             |
| `routes/properties.rs`| ✅ M6  | `GET/PUT /api/v1/servers/{id}/properties`, redaction + hash checks    |
| `properties.rs`       | ✅ M6  | Java properties parse/serialise, escape handling, redaction helpers   |
| `routes/ws.rs`        | ✅ M4  | `/api/v1/ws/servers/{id}`: JWT query/subprotocol, daemon fan-in      |
| `routes/v1.rs`        | ✅ M2  | authenticated probe `GET /api/v1/me` + server routes                |
| `daemon.rs`           | ✅ M6  | HMAC-signed client incl. streaming GET, RCON dispatch, file read/write |
| `middleware.rs`       | ✅ M2  | JWT guard + forced-password-change gate (only `/me` allowed)        |
| `auth.rs`             | ✅ M2  | argon2id hashing/verification, JWT issue/verify, rotation flag      |
| `bootstrap.rs`        | ✅ M2  | seed the first admin flagged `requires_password_change = 1`         |
| `state.rs`            | ✅ M2  | `AppState { pool, auth, daemon, config }`, pool + migrations        |
| `error.rs`            | ✅ M2  | `ApiError` (`thiserror`) → `IntoResponse` with `ApiErrorBody`        |
| `telemetry.rs`        | ✅ M1  | tracing subscriber, JSON via `LOG_FORMAT=json`                      |
| `store/`              | later  | extracted non-auth queries, transactional port allocation           |
| `jobs/`               | later  | durable job rows, progress fan-out, cancellation                    |
| `ws/`                 | later  | multi-topic hub, snapshots + sequence gaps (per-server socket ✅ M4) |
| `nodes/`              | M10    | node registry, heartbeats, multi-node dispatch                      |
| `rbac/`               | M7     | roles/permissions layered over the existing auth extractor          |

Async model: every request is a tokio task; shared state is `Arc<AppState>`; background workers
(heartbeat timeout, job scheduler, metrics aggregation) are spawned with `tokio::spawn` and
stopped via `CancellationToken` on shutdown. The automation worker (`scheduler.rs`) starts with
the API, sleeps 5 s, then claims due rows every 20 s by advancing `next_run_at` before executing
them.

### 4.3 Node daemon — `backend/crates/daemon` (`cluster-daemon`)

Responsibilities: own the Docker socket, translate intents into container operations, stream
logs/metrics, read/write files inside the server volume, run backups, expose a local control
channel to the control plane.

Cargo dependencies: `bollard 0.21` (Docker Engine client), `axum` (control channel), `tokio`,
`futures-util`, `tokio-stream`, `serde`, `tracing`, `thiserror`, `uuid`, `time`, `sha2`, `hex`,
`zip` (deflate).

| Module (implemented)  | Responsibility                                                    |
| --------------------- | ----------------------------------------------------------------- |
| `config.rs`           | env config, `MC_DATA_ROOT` canonicalisation                       |
| `docker.rs`           | bollard connect/ping, network ensure, container create/start/stop/remove |
| `minecraft.rs`        | pure spec → env + `HostConfig`; RCON port/password derivation     |
| `rcon.rs`             | Source RCON framing, auth/exec client, credential resolution      |
| `files.rs`            | sandboxed path validation, atomic text read/write, SHA-256 hashes |
| `stats.rs`            | Docker stats → `StatsSample`; CPU delta, working set, network sums |
| `logs.rs`             | multiplexed log frames → timestamped `LogLine`s, ANSI stripping   |
| `imports.rs`          | ZIP/host-path import, staging, traversal-safe extraction          |
| `modpacks.rs`         | `.mrpack` download + verified override extraction                 |
| `backups.rs`          | ZIP snapshots, SHA-256 manifests, verified restore, deletion      |
| `players.rs`          | access-list reads/writes (`ops`, `whitelist`, `banned-*`)         |
| `telemetry.rs`        | NDJSON framing helpers for stats/log streams                      |
| `api/auth.rs`         | HMAC verification middleware (timestamp window + constant-time)   |
| `api/routes.rs`       | `/daemon/servers/*` lifecycle, command, streams, files, add-ons, imports, backups |
| `error.rs`            | `DaemonError` → `ApiErrorBody`, Docker status mapping             |

Planned for later milestones: a persistent `agent.rs` heartbeat channel and multi-node
reconciliation.

### 4.4 Docker integration — see §6.

### 4.5 MCP server — `mcp/`

A standalone Node 20+ process that speaks the Model Context Protocol over stdio. It is the
repository's **markdown context protocol** integration: every markdown file (README, `docs/*`,
`deployment/README.md`, `mcp/README.md`) is registered as a `cluster://docs/<path>` resource with
`text/markdown`, re-read on each request. Tools wrap the REST API with a JWT-authenticated client
(`CLUSTER_API_TOKEN`, or lazy login from `CLUSTER_API_USERNAME`/`CLUSTER_API_PASSWORD` with one
transparent re-login after a 401):

| Area | Tools |
| ---- | ----- |
| Docs | `list_documents`, `read_document` |
| Servers | `list_servers`, `get_server`, `create_server`, `server_power` |
| Backups | `list_backups`, `create_backup`, `restore_backup`, `delete_backup`, `verify_backup`, `get_backup_retention`, `set_backup_retention` |
| Schedules | `list_schedules`, `create_schedule`, `update_schedule`, `run_schedule`, `delete_schedule` |
| Catalog | `describe_runtime` (plugins/mods/modpacks + Modrinth loaders) |

Destructive tools carry MCP annotations and `restore_backup` creates a safety backup by default.
The MCP server has no port and no storage of its own; it has exactly the authority of its token,
so it is not exposed by the deployment Nginx config.

## 5. Data ownership

| Data                             | Source of truth                    |
| -------------------------------- | ---------------------------------- |
| Users, nodes, server records     | control-plane SQLite (`control.db`) |
| Proxy networks + membership      | control-plane SQLite (`proxy_networks`, `servers.proxy_network_id`) |
| Port assignments                 | control-plane SQLite (unique index) |
| Live container state, logs       | node daemon → Docker               |
| World, configs, player data      | server volume on the node          |
| Backup archives                  | node disk (`<MC_DATA_ROOT>/<id>/backups/*.zip`) |
| Backup metadata + schedules      | control-plane SQLite (`backups`, `scheduled_tasks`) |
| Player display cache             | control-plane (derived, rebuildable) |

The control plane stores **desired state**; the daemon reports **observed state**. Status shown in
the UI is `observed`, with `desired ≠ observed` surfaced as "reconciling". This avoids the classic
bug of a dashboard claiming "running" for a crashed container.

## 6. Docker strategy

### 6.1 Image — D1 (approved)

Use `itzg/minecraft-server` rather than a bespoke image. It handles
Vanilla/Paper/Fabric installation, Java version selection, EULA, RCON and graceful shutdown,
saving weeks of work. The image ref is configurable via `MC_IMAGE` (default
`itzg/minecraft-server:latest`); production deployments should pin it by digest. Bedrock uses
`MC_BEDROCK_IMAGE` and Velocity/BungeeCord/Waterfall use `MC_PROXY_IMAGE`
(default `itzg/mc-proxy:latest`), because those families are separate images in the itzg ecosystem.

We wrap it, not depend on it blindly:

- Everything image-specific sits behind the pure translation in
  `cluster-daemon/src/minecraft.rs` (`env_for`, `host_config`, `create_body`). Swapping to our own
  image later touches one module.
- M2 sets only `EULA`, `TYPE`, `MEMORY`, `SERVER_NAME`, `OVERRIDE_SERVER_PROPERTIES=false`, plus
  `ONLINE_MODE=false` for proxy backends. M5 adds `ENABLE_RCON=TRUE`, `RCON_PORT=25575` and a
  per-server `RCON_PASSWORD`; later milestones add `VERSION`, world fields, `ARCLIGHT_TYPE` plus a
  daemon-resolved `ARCLIGHT_RELEASE` tag for Arclight, and daemon-resolved `CUSTOM_SERVER`/
  `MAGMA_VERSION` for CatServer and Magma.
- `OVERRIDE_SERVER_PROPERTIES=false` — we edit the file through the config editor instead, so the
  image never fights our writes.
- Version pinning per server is explicit. Normal Java runtimes accept `LATEST`; Arclight,
  CatServer, Mohist and Magma do **not** publish builds for every Mojang release and therefore
  require a concrete supported version. `GET /api/v1/runtimes/{type}/versions` resolves the
  runtime's own release index (GitHub, Mohist API, Magma API or the Mojang manifest) and the API
  rejects `LATEST`/`SNAPSHOT` for those four runtimes.

### 6.2 Container contract

- Name: `mc-<serverId>`; labels `console.managed=true`, `console.server=<id>`,
  `console.type=<kind>` — listing/cleanup is label-driven, never name parsing.
- Volume: `<MC_DATA_ROOT>/<serverId>/data:/data` (bind mount, not a named volume, so backups and
  file edits are plain host paths and survive Docker resets); proxies mount the same host directory
  at `/server`, the mc-proxy image's home. `MC_DATA_ROOT` is canonicalised at
  daemon startup because Docker requires an absolute bind source.
- Restart policy: `unless-stopped`, so daemon restarts don't kill worlds.
- Security: `no-new-privileges`, `PidsLimit=512`, stop grace period 35 s. Healthchecks and an
  RCON-driven graceful stop sequence remain future hardening.

### 6.3 Resource limits

| Knob            | Value                                                              |
| --------------- | ------------------------------------------------------------------ |
| Memory limit    | user-selected (slider 1–16 GB), `MemorySwap == Memory` (no swap)   |
| Heap            | `-Xms`/`-Xmx` = `clamp(1G, 65% of limit, limit − 512M)`, rounded to 256M |
| CPU             | `NanoCPUs` = 1.0–4.0 cores                                          |
| PIDs            | 512                                                                 |
| Disk            | soft quota via periodic volume usage check; hard quota in v2        |

Enforcement is Docker's, implemented in `host_config`: `Memory == MemorySwap == ram_mb` (no swap),
`NanoCPUs = cpu_limit × 1e9`, `PidsLimit = 512`, `no-new-privileges`. The API validates
`ram_mb ∈ [512, 65536]` and `cpu_limit ∈ (0, 32]` before dispatch; the daemon re-validates rather
than trusting the control plane. Node-capacity admission control arrives with multi-node (M10).

### 6.4 Networking

- Dedicated user-defined bridge `MC_NETWORK` (default `mc-net`), created idempotently by the daemon.
  No `--network host`.
- Standalone server: `PortBindings` maps `0.0.0.0:<game_port>` → `25565/tcp`, and the container
  exposes `25565/tcp` on the bridge network. Proxies publish the same host port to
  `25577/tcp` (mc-proxy's BungeeCord/Waterfall listener); Velocity publishes `25565/tcp`
  because that is its own default; Bedrock publishes UDP `19132`.
- RCON is published **only on the loopback**: `127.0.0.1:<derived 30000–39999>` → `25575/tcp` for
  every server. The daemon connects to `127.0.0.1`, which works on Linux VPS hosts and Docker
  Desktop (where container IPs are not routable from the host) while remaining unreachable
  externally. The host port is deterministic from the server id; collisions are unlikely at v1
  scale and would be removed by a future stored port allocator.
- `online-mode` defaults to `true`; offline mode is an explicit, flagged toggle (it is a security
  decision, not a preference).

### 6.5 Proxy networks — Velocity / BungeeCord (optional)

A proxy is **not** a special daemon feature: it is a managed container like any other, with
`server_type = velocity | bungeecord | waterfall`, running on the dedicated `itzg/mc-proxy` image
(`MC_PROXY_IMAGE`) which supports `TYPE=VELOCITY`, `TYPE=BUNGEECORD` and `TYPE=WATERFALL`.

Grouping and routing:

- `proxy_networks` groups one proxy with its backend servers. Proxy containers also carry a
  `proxy_network_id`, so the group is discoverable from either side.
- **Port exposure rule:** when `is_behind_proxy = 0`, the container gets
  `PortBindings { "<game container port>/tcp": 0.0.0.0:<game_port> }`. When `is_behind_proxy = 1`,
  `PortBindings` is omitted entirely: the container still lists its game port (25565 for Java
  servers and Velocity, 25577 for BungeeCord/Waterfall) in `ExposedPorts`, so it is reachable by
  other containers on `mc-net`, but **nothing is bound on the host**. Verified by inspecting the
  created container: proxied backend → `PortBindings = {}`; standalone → `0.0.0.0:<port>`.
- Backends in a proxy network run `online-mode=false` (Velocity/BungeeCord performs auth) and must
  use player-info forwarding:
  - Velocity: `player-info-forwarding-mode=modern` + shared `forwarding.secret`, backends use
    `paper-global.yml` `velocity.enabled=true` and the same secret.
  - BungeeCord: `bungeecord: true` on Paper/Spigot backends + `ip_forward: true` on the proxy.
- The forwarding secret is generated per network, stored server-side, written to the proxy and
  backends by the daemon, and never returned by the API. (Future proxy milestone.)
- Firewall/porting check: all backend game ports must be unpublished (or bound to `127.0.0.1`) as
  well as firewalled, otherwise clients can bypass the proxy and its auth.
- Enabling/disabling the proxy toggle on a backend requires a restart; the API reports
  `restart_required` and the UI forces an explicit confirmation because offline-mode is a security
  change.
- Single-node v1 does not need `mc-router`; the proxy owns the public port. Hostname-based routing
  across many proxies/protocols remains a later option (§13).

### 6.6 Security boundaries

| Principal        | May                                                          |
| ---------------- | ------------------------------------------------------------ |
| Browser          | authenticated REST + WS only                                 |
| Control plane    | DB, daemon control channel; **no** Docker socket, no host FS |
| Daemon           | Docker socket, server data root, backup root                 |
| Game container   | only its own `/data`, no host network, no privileges         |

**D4 (accepted): HMAC-SHA256 with a shared secret over TLS; no mTLS in v1.** Request canonical
string:

```text
METHOD
/path?query
unix_timestamp
hex(sha256(body))
```

The control plane sends `x-cluster-timestamp` and `x-cluster-signature`
(`hex(HMAC-SHA256(secret, canonical))`). The daemon reads the raw body, verifies with
`mac.verify_slice` (constant-time), and rejects missing/malformed headers, wrong secrets, tampered
method/path/body, and timestamps more than **300 s** from its clock. The window bounds replay without
a nonce store; TLS protects the secret and payload in transit. `DAEMON_SHARED_SECRET` must match on
both processes. Containers run with `no-new-privileges`; read-only root filesystems are evaluated
against the image in M7.

**File sandbox (M6).** The daemon is the only component allowed to touch server files. Every path is
interpreted relative to `<MC_DATA_ROOT>/<server_id>/data`; absolute paths, `..`, NUL bytes and empty
paths are rejected before any filesystem call. Existing targets and every parent component are
canonicalised, then checked with `starts_with(canonical_root)`, so a symlink cannot redirect a read
or write outside the volume. Writes are capped at 1 MiB and use `temp file → fsync → rename` in the
same directory. The control plane's file routes are fixed to `server.properties`; there is no
user-supplied path on the browser API.

## 7. Key data flows

**Create server (M2, synchronous)**
`POST /api/v1/servers` → validate name/type/port/limits + proxy rules + world fields
(`world_name` is a single safe path component; seed length; world type/difficulty/gamemode are
enumerated) → insert `servers` row with `status = provisioning` (unique name/port indexes prevent
races) → sign and send `POST /daemon/servers/{id}/create` → daemon ensures `mc-net`, creates the
volume, creates the container → on success the control plane sets `status = stopped`; on daemon
failure the row is deleted (compensation) and the error is returned. `POST /{id}/start|stop`
forwards to the daemon and then updates `status`. A durable `jobs` table and async progress events
remain future work; current calls are synchronous and therefore bounded by the daemon client's
30 s timeout. The wizard mirrors this contract (runtime → world → resources → details); an optional
data ZIP is then sent to `POST /api/v1/servers/{id}/import-data`, which requires `stopped | error`
and streams a temporary file to the daemon's import endpoint.

**Create proxy network**
`POST /api/v1/proxy-networks {name, kind}` → insert network → create the proxy container
(`server_type = velocity | bungeecord`) → daemon generates the forwarding secret, writes proxy and
backend configs → backends are marked `is_behind_proxy = 1`, their published ports are removed and
they restart in offline mode. Route tables in the proxy UI map hostnames → backend `game_port`.
(M2 only validates that a referenced `proxy_network_id` exists; there is no CRUD endpoint yet.)

**Power action (implemented)**
`POST /api/v1/servers/{id}/start|stop|restart` → control plane loads the row, forwards the current
`ram_mb`/`cpu_limit` (start/restart) to the daemon → daemon applies limits and drives Docker →
status is updated on success. Calls are synchronous and bounded by the daemon client's 30 s timeout;
there is no durable job row or 202/job-ID protocol yet. The UI disables conflicting actions while a
request is in flight and never shows a fake success.

**RCON command (M5)**
`POST /api/v1/servers/{id}/command {command}` → validate (≤1024 chars, no control characters,
optional leading slash stripped) → require `status = running` → signed
`POST /daemon/servers/{id}/command` → resolve credentials from the container env, then
`server.properties`, then the derived password → connect to `127.0.0.1:<rcon_host_port>` → Source
RCON auth (`id=1`) → exec (`id=2`) → return `{command, output}`. Semantic player routes
(`players/{username}/kick|ban|op`) validate the username (`[A-Za-z0-9_]{1,16}`) and reason
(≤256 chars, no controls), then translate to `kick|ban|op <username> [reason]` before using the
same path. Injection attempts are rejected with `400`; a disabled RCON or unreachable daemon is
reported as `503 dependency_unavailable`.

**Live player roster (M5-Part 1)**
The UI ships the full roster/inspection layout with preview players. Authoritative roster and
vitals arrive with the companion plugin (M5-Part 2): a 2 s `list` poll over RCON, event-driven
refresh on join/leave console lines, and NBT/plugin reads for health, hunger, XP and inventory.
Per-player stats come from NBT reads when offline, and from the companion plugin when online (§9).

**Config edit (M6)**
`GET /api/v1/servers/{id}/properties` → control plane signs `GET /daemon/servers/{id}/files/server.properties`
→ daemon validates the path, canonicalises it under the server volume, reads UTF-8 (≤1 MiB) and
returns `{path, content, hash}` → API parses the Java properties format into ordered
`comment | blank | property` entries, redacts `rcon.password`, and returns the entries plus hash.
The UI renders schema controls. `PUT /api/v1/servers/{id}/properties {entries, expected_hash}`
re-reads the file, restores redacted values from disk, validates and serialises the entries, then
signs `PUT /daemon/servers/{id}/files/server.properties`. The daemon rejects a stale hash with `409`,
writes atomically (temp file, same dir, `fsync`, `rename`), and returns the new hash; the UI shows a
restart-required modal. Comments, blank lines, ordering and unknown keys survive the round-trip.

**Backup (implemented)**
`POST /api/v1/servers/{id}/backups` → daemon checks container state → if running: RCON `save-off`
then `save-all flush` (best-effort) → ZIP the whole `<data>/` volume to
`<MC_DATA_ROOT>/<id>/backups/<uuid>.zip` with deflate in a blocking worker, skipping symlinks and
enforcing entry/size caps → hash the archive (SHA-256) → write `<uuid>.json` manifest → RCON
`save-on` → control plane catalogs the row. Listing reads the daemon manifests and reconciles the
catalog. Restore = verify manifest hash → extract to a staging directory with traversal checks →
rename the live `data/` aside → rename staging into place → restart the container if it was running;
the daemon attempts the restart even if the restore fails. Delete removes the archive and manifest.
Scheduled backups use the same daemon endpoint through the worker.

`GET /backups/{id}/download` streams the archive from the daemon in 64 KiB chunks through the
signed GET channel; neither process buffers the ZIP in memory. `POST /backups/{id}/verify`
re-hashes the archive and compares it with the manifest without touching the running server.
Retention lives in `backup_settings` (migration `0006`): a per-server `retention_count` of
`0` disables pruning, otherwise the control plane keeps the newest N archives and deletes the
rest after each successful manual or scheduled backup. Pruning is best effort and cannot fail
the snapshot that triggered it.

**Scheduled task (implemented)**
Rows in `scheduled_tasks` hold `interval_minutes`, `enabled`, `next_run_at`, `last_run_at`,
`last_status` and `last_error`. A single API worker polls every 20 s, selects up to 20 due rows,
advances `next_run_at` **before** spawning execution (so a slow task is not claimed twice), then
runs `backup | start | stop | restart | command` through the same daemon client as HTTP requests.
Backups are cataloged; failures are recorded on the row and logged. A crash during execution means
the task retries on its next interval; there is no durable job queue yet. "Run now" executes the
same function synchronously and returns the refreshed row.

## 8. Real-time protocol

> **Status: implemented in M4.** One WebSocket per open server detail view. The future multi-topic
> hub (node metrics, job progress, player roster) will keep this envelope style.

### 8.1 Daemon → control plane (HMAC NDJSON)

Two signed GET streams complement the lifecycle endpoints:

| Method | Path | Body |
| ------ | ---- | ---- |
| GET | `/daemon/servers/{id}/stats` | `application/x-ndjson`, one `StatsSample` per line |
| GET | `/daemon/servers/{id}/logs` | `application/x-ndjson`, one `LogLine` per line |

Both use the existing canonical HMAC signature with an empty body and are served by axum
`Body::from_stream`. Docker's `stats(stream=true)` drives the stats stream; `logs(follow=true,
timestamps=true, tail=200)` drives the log stream. Multi-line Docker log frames are split and the
RFC 3339 prefix is removed before encoding. Bounded `mpsc` channels apply backpressure, and closing
the HTTP response stops the Docker polling task.

The API's daemon client intentionally has **no total request timeout** on these streams. Its
per-request 30 s timeout applies to ordinary control calls only; applying it to a long-lived NDJSON
response made reqwest abort every stats/log stream after 30 s with
`error decoding response body`. The client keeps a 10 s connect timeout instead.

Missing optional paths are not treated as failures: listing an absent `plugins`, `mods`,
`resourcepacks` or `<world>/datapacks` directory returns an empty list, and daemon `404` responses
are logged at debug level. RCON `Connection refused` immediately after a start is translated to a
retryable `409 Conflict` ("server is still starting") instead of a `503` error-level log.

`StatsSample` contains `cpu_percent` (normalised so `online_cpus * 100` is full machine load),
`memory_used_bytes` (working set: usage minus reclaimable page cache), `memory_limit_bytes`,
`memory_percent`, `network_rx_bytes`, `network_tx_bytes`, `pids`, `timestamp_ms`.

### 8.2 Browser WebSocket

`GET /api/v1/ws/servers/{id}` upgrades with axum. Browsers cannot set `Authorization` on the
upgrade request, so the JWT is accepted as `?token=<jwt>` (used by the SPA) or as a
`bearer.<jwt>` subprotocol (useful for non-browser clients). The handler validates the token,
rejects users with a pending password change, and confirms the server exists before upgrading —
missing servers stay normal REST `404` responses.

Each message is one JSON envelope with an internal `type` tag:

```json
{ "type": "ready", "server_id": "01H…" }
{ "type": "stats", "sample": { "cpu_percent": 71.2, "memory_used_bytes": 3_000_000, "memory_limit_bytes": 1_073_741_824, "memory_percent": 0.27, "network_rx_bytes": 1752, "network_tx_bytes": 126, "pids": 3, "timestamp_ms": 1794640059059 } }
{ "type": "log", "line": { "stream": "stdout", "timestamp_ms": 1794640059059, "message": "[12:00:02] [Server thread/INFO]: Steve joined the game" } }
{ "type": "stream_ended", "stream": "stats" }
{ "type": "error", "code": "daemon_stream_unavailable", "message": "container is not running" }
```

- The control plane opens both daemon streams, splits NDJSON, re-wraps each item, and multiplexes
  them through one bounded `tokio::sync::mpsc` channel into the socket sender loop.
- Ping frames are answered with Pong; a browser Close aborts both forwarding tasks, which closes
  the daemon responses.
- If a daemon stream fails, the socket stays open and emits `error` + `stream_ended`, so logs can
  keep flowing when stats are unavailable (for example while a container is stopped).
- Commands remain **REST**. The WebSocket is outbound telemetry only; there is no console input
  yet. Retries, validation and audit stay in one place.
- The frontend reconnects with capped exponential backoff (1 s → 10 s) while the server is running,
  and keeps a 600-line log ring buffer.

### 8.3 Graphical console parsing

`frontend/src/lib/logs.ts` turns a raw line into `{ time, source, level, tone, message }`:

- Strips an optional `[HH:MM:SS]` prefix, then matches `[<source>/<LEVEL>]:` (Vanilla, Paper,
  Fabric, proxies). Unrecognised lines degrade to a `system` row.
- Classifies tones: `ERROR/FATAL/SEVERE` → red, `WARN` → amber, `DEBUG/TRACE` → dim,
  `joined the game | has connected` → green join, `left the game | has disconnected` → muted leave,
  `<player> message` → cyan chat, stack-trace/exception/failure wording → red.
- The UI renders one row per line with a timestamp, level badge and tone-specific background;
  filter chips (All / Info / Warnings / Errors / Players) operate on the parsed tones. Auto-scroll
  pauses when the user scrolls up and offers “jump to latest”.

## 9. Minecraft-specific mechanics

- **server.properties** — parsed line-by-line preserving comments, ordering and unknown keys; a
  schema table drives rendering (bool → toggle, int → slider, enum → select,
  `restart_required` → UI badge).
- **Whitelist / ops / bans** — typed JSON models; edits are diffed and written atomically, then
  applied live via RCON (`whitelist reload`, `op`, `deop`, `ban`, `pardon`).
- **Player data** — `world/playerdata/<uuid>.dat` is gzipped NBT. Reading is always safe. Writing
  is only safe while the player is offline (`OfflinePlayer`), or the server overwrites it at the
  next autosave.
- **D2 (approved)** — a companion Paper plugin owns authoritative live edits: health, hunger,
  exact XP, inventory slots. It runs inside the server thread, so values are consistent and cannot
  be clobbered by autosave. The daemon talks to it over a small socket on `mc-net`; the plugin is
  versioned alongside the manager. Fallback without the plugin is documented as "best effort"
  (RCON `/effect`, `/xp`, `/give`, `/clear`, `/item replace`, offline NBT edits) and the UI hides
  controls the fallback cannot honour.
- **RCON** — primary control channel (console, list, save, whitelist). A unique password is derived
  per server from `DAEMON_SHARED_SECRET` (64 hex chars), injected as `RCON_PASSWORD` at create time,
  and never sent to the browser. Containers created before M5 fall back to `server.properties`; if
  neither source enables RCON the daemon returns a clear "recreate the container" error. RCON is
  published on the host loopback only (§6.4).

## 10. Data model

### 10.1 Implemented migrations

Migration `0001_init.sql`:

```sql
users (
  id            TEXT PRIMARY KEY,                 -- UUIDv7
  username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash TEXT NOT NULL,                    -- argon2id PHC string
  created_at    TEXT NOT NULL                     -- ISO-8601 UTC
)

proxy_networks (
  id         TEXT PRIMARY KEY,                    -- UUIDv7
  name       TEXT NOT NULL UNIQUE,
  proxy_kind TEXT NOT NULL CHECK (proxy_kind IN ('velocity', 'bungeecord')),
  created_at TEXT NOT NULL
)

servers (
  id               TEXT PRIMARY KEY,              -- UUIDv7
  name             TEXT NOT NULL UNIQUE COLLATE NOCASE,
  game_port        INTEGER NOT NULL UNIQUE CHECK (game_port BETWEEN 1 AND 65535),
  server_type      TEXT NOT NULL CHECK (server_type IN
                     ('vanilla','paper','fabric','velocity','bungeecord')),
  ram_mb           INTEGER NOT NULL CHECK (ram_mb >= 512),
  cpu_limit        REAL NOT NULL CHECK (cpu_limit > 0.0),
  is_behind_proxy  INTEGER NOT NULL DEFAULT 0 CHECK (is_behind_proxy IN (0, 1)),
  proxy_network_id TEXT REFERENCES proxy_networks(id) ON DELETE SET NULL,
  status           TEXT NOT NULL DEFAULT 'stopped' CHECK (status IN
                     ('provisioning','stopped','starting','running','stopping','error')),
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL                  -- maintained by trigger
)
```

Migration `0002_force_password_change.sql`:

```sql
ALTER TABLE users
  ADD COLUMN requires_password_change INTEGER NOT NULL DEFAULT 0
  CHECK (requires_password_change IN (0, 1));
UPDATE users SET requires_password_change = 1;    -- existing/seeded users must rotate
```

The flag is embedded in the JWT (`AuthUser.requires_password_change`). `require_auth` rejects every
protected route except `GET /api/v1/me` with `403 password_change_required` until
`POST /api/auth/change-password` succeeds; that route uses a variant middleware that validates the
token but ignores the flag. A successful rotation clears the column and returns a fresh token.

Migration `0003_forge_and_game_version.sql` rebuilds `servers` to add `forge` and
`game_version TEXT NOT NULL DEFAULT 'LATEST'` (SQLite cannot widen a CHECK constraint in place).

Migration `0004_server_catalog.sql` expands the runtime CHECK to **19** types (`vanilla`, `paper`,
`fabric`, `forge`, `velocity`, `bungeecord`, `bedrock`, `neoforge`, `quilt`, `spigot`, `purpur`,
`pufferfish`, `folia`, `sponge`, `mohist`, `arclight`, `catserver`, `magma`, `waterfall`) and adds
`world_name`, `world_seed`, `world_type`, `difficulty`, `gamemode` with defaults.

Migration `0005_backups_and_schedules.sql` adds:

```sql
backups (
  id TEXT PRIMARY KEY, server_id TEXT REFERENCES servers(id) ON DELETE CASCADE,
  name TEXT, size_bytes INTEGER, sha256 TEXT, server_was_running INTEGER,
  created_at_ms INTEGER, created_at TEXT
)

scheduled_tasks (
  id TEXT PRIMARY KEY, server_id TEXT REFERENCES servers(id) ON DELETE CASCADE,
  name TEXT, task_type TEXT CHECK (task_type IN
    ('backup','start','stop','restart','command')),
  command TEXT, interval_minutes INTEGER CHECK (1..525600),
  enabled INTEGER, next_run_at TEXT, last_run_at TEXT,
  last_status TEXT CHECK (last_status IN ('success','error')), last_error TEXT,
  created_at TEXT, updated_at TEXT
)
```

Migration `0006_backup_settings.sql` adds the per-server retention policy:

```sql
backup_settings (
  server_id TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
  retention_count INTEGER NOT NULL DEFAULT 0 CHECK (retention_count BETWEEN 0 AND 500),
  updated_at TEXT
)
```

Indexes cover backup timelines (`server_id, created_at_ms DESC`), due tasks
(`enabled, next_run_at`) and per-server schedules. The archive bytes stay on the node; the row is a
catalog for fast UI listing and survives daemon reconciliation.

Indexes: `servers(proxy_network_id)`, `servers(status)`. A trigger advances `updated_at` on every
update. Foreign keys are enabled on each connection.

Notes:

- `is_behind_proxy = 1` means "do not publish a public game port"; the API layer enforces that such
  a server must reference a `proxy_network_id` (not a DB constraint, so a server can be created
  before its network is assigned). It also forbids a `velocity`/`bungeecord` proxy from being behind
  a proxy.
- `proxy_network_id` is optional and `ON DELETE SET NULL`, so removing a network never deletes
  servers.
- **Deferred:** `nodes`, `sessions`, `server_revisions`, `players`, `jobs`, `audit_log` arrive with
  their milestones. The single-node v1 does not need `servers.node_id` yet; adding it later is an
  additive migration (`ALTER TABLE servers ADD COLUMN node_id` + backfill). Backups and scheduled
  tasks are now implemented as described above.
- IDs are UUIDv7 (time-sortable) generated in Rust, stored as TEXT. Timestamps are UTC ISO-8601.

### 10.2 SQLite settings

WAL journal, `foreign_keys=ON`, `busy_timeout=5s`, `synchronous=NORMAL`, pool capped at 5
connections, migrations embedded via `sqlx::migrate!`. `_sqlx_migrations` records applied versions.

## 11. Deployment

- The current single-VPS deployment is documented in `deployment/README.md`: Nginx serves the
  built SPA from `/var/www/cluster-panel`, protects the panel with HTTP Basic auth, and proxies
  `/api/` (including WebSocket upgrades) to the API on loopback. Systemd runs the API as the
  `cluster` service account and the daemon as root with Docker group access.
- Both Rust processes bind to loopback; the daemon owns the Docker socket and Minecraft volumes.
  SQLite is stored under `/var/lib/cluster-console`; server data is under `/srv/minecraft`.
- The supplied Nginx site listens on port 80. Add TLS before exposing it to the public internet;
  nightly **database** snapshots and off-host backup sync remain future operations work (per-server
  data backups are implemented and stay on the node disk).
- The MCP server runs on the operator's workstation over stdio and is **not** deployed to the VPS;
  it reaches the API through Nginx or an SSH tunnel using a JWT.
- Health: `/healthz` (liveness) and `/readyz` (DB + node reachability) on both binaries.
- Observability: `tracing` structured logs (JSON via `LOG_FORMAT=json`), request IDs end-to-end,
  Prometheus `/metrics` in v1.1.

## 12. Decision log

| ID  | Decision                                                              | Status   |
| --- | --------------------------------------------------------------------- | -------- |
| D0  | **Backend language: Rust** — axum for HTTP/WS, bollard for Docker      | accepted |
| D1  | Wrap `itzg/minecraft-server` behind a narrow translation module (image ref configurable) | accepted |
| D2  | Companion Paper plugin for authoritative live stat/inventory edits     | accepted |
| D3  | SQLite via `sqlx` for v1; PostgreSQL when multi-replica becomes real   | accepted |
| D4  | Node auth via HMAC-SHA256 with a shared secret over TLS now, mTLS later | accepted |
| D5  | REST for commands, WS for streams only                                 | accepted |
| D6  | Bind mounts (not named volumes) for `/data`                            | accepted |
| D7  | No Docker socket in the control plane                                  | accepted |
| D8  | Single-node v1; node registry lands with multi-node (additive migration) | accepted |
| D9  | Local auth only: argon2id passwords, JWT (HS256) session tokens        | accepted |
| D10 | Rust workspace: `cluster-api`, `cluster-daemon`, `cluster-shared`      | accepted |
| D11 | Proxies are managed containers (`velocity`/`bungeecord`) grouped by `proxy_networks`; backends behind a proxy do not publish a port | accepted |
| D12 | First-run admin is seeded only when `users` is empty; password from `ADMIN_PASSWORD`, otherwise random and logged once; a password change is forced at first login | accepted |
| D13 | Config editing round-trips ordered `server.properties` entries, uses SHA-256 optimistic concurrency, and redacts `rcon.password` from the browser while restoring it on save | accepted |
| D14 | Backups are ZIP archives with SHA-256 sidecar manifests on the node; recurring tasks are SQLite rows executed by an in-process 20 s worker | accepted |
| D15 | The markdown context protocol is implemented as an MCP server exposing repository markdown as resources and the REST API as typed tools | accepted |

## 13. Open questions for review

1. Version matrix policy: latest release only, or a browsable catalog per server kind?
2. Companion plugin transport: tiny TCP/JSON socket (proposed) vs. embedding a second HTTP server
   inside the plugin.
3. Proxy vs. `mc-router`: the proxy owns the public port in v1. Do we later add hostname routing
   in front of multiple proxies, or keep one public port per proxy?
