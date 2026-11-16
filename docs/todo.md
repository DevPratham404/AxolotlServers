# TODO — Cluster Console

Checkbox discipline: only check an item when it is verified, not when it is written.
Milestones were renumbered when M1 was narrowed to "control-plane foundation" (auth + schema).

## M0 — Scaffold & pivot ✅

- [x] `docs/` with `architecture.md`, `state.md`, `todo.md`
- [x] Vite + React + TypeScript + Tailwind 4 frontend shell, design tokens, `/api` + `/ws` proxy
- [x] Verify `npm run build`
- [x] Remove Go scaffold and legacy TypeScript backend; clean commit
- [x] Rust workspace: `cluster-api`, `cluster-daemon`, `cluster-shared`
- [x] `cargo check` / `cargo build` / `cargo clippy -D warnings` clean

## M0.5 — Decisions

- [x] D0 Rust backend (axum + bollard)
- [x] D1 Wrap `itzg/minecraft-server`
- [x] D2 Companion Paper plugin for authoritative player writes
- [x] D3 SQLite via `sqlx`
- [x] D8 Single-node v1, node registry deferred
- [x] D9 Local auth: argon2id + JWT
- [x] D11 Proxy networks (Velocity/BungeeCord) as first-class containers
- [x] D12 First-run admin bootstrap
- [x] D4 daemon auth: HMAC-SHA256 shared secret over TLS; mTLS deferred to v1.1
- [x] Force password change on first admin login (`requires_password_change`, gated routes)

## M1 — Control-plane foundation ✅

**Persistence**

- [x] SQLite pool: `create_if_missing`, WAL, `foreign_keys=ON`, `busy_timeout=5s`, 5 connections
- [x] Embedded migration runner (`sqlx::migrate!("../../migrations")`)
- [x] Migration 0001: `users`, `proxy_networks`, `servers` (+ indexes, `updated_at` trigger)
- [x] `servers` CHECK constraints: port range, server types including `velocity`/`bungeecord`,
      `ram_mb >= 512`, `cpu_limit > 0`, status enum
- [x] `servers.proxy_network_id` FK (`ON DELETE SET NULL`), `is_behind_proxy` flag

**Bootstrap**

- [x] Seed default admin only when `users` is empty (idempotent)
- [x] Password from `ADMIN_PASSWORD`, else random 24-char and logged once
- [x] argon2id hash computed off the async runtime (`spawn_blocking`)

**Auth**

- [x] `AuthService`: HS256 issue/verify, `exp` + `sub` required, algorithm pinned
- [x] `POST /api/auth/login` → `{token, token_type, expires_at, user}`
- [x] Uniform 401 for unknown user and wrong password (no enumeration)
- [x] `require_auth` middleware: Bearer parsing, JWT validation, `AuthUser` extension
- [x] Protected `/api/v1` router; `GET /api/v1/me` proves the chain
- [x] `ApiError` → consistent `ApiErrorBody`, `WWW-Authenticate: Bearer` on 401
- [x] `/readyz` now checks SQLite and returns 503 when unavailable
- [x] Request ID propagation + trace layer
- [x] Unit tests: password round-trip, JWT round-trip/rejection, Bearer parsing
- [x] `cargo check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo fmt` all clean
- [x] Manual E2E: login 200, bad password 401, unknown user 401, `/me` 200 with token,
      401 without/tampered token; tables verified with `sqlite3`

## M2 — Docker orchestration & server lifecycle ✅ (backend)

**Decisions**

- [x] D4 resolved: HMAC-SHA256 shared secret over TLS, no mTLS in v1

**Password rotation**

- [x] Migration 0002: `users.requires_password_change` (0|1, default 0)
- [x] Bootstrap admin seeded with `requires_password_change = 1`
- [x] Claim/`AuthUser` carries the flag; login exposes it
- [x] `require_auth` blocks all `/api/v1/*` except `GET /me` while pending (`403 password_change_required`)
- [x] `POST /api/auth/change-password` verifies current password, enforces ≥ 8 chars, clears flag,
      returns a fresh token
- [x] Manual E2E: login → 403 on servers → rotate → 200 on servers

**Daemon link + HMAC**

- [x] Shared canonical request: `METHOD\npath?query\ntimestamp\nhex(sha256(body))`
- [x] `cluster-shared::hmac_auth` sign/verify with constant-time comparison; 300 s skew window
- [x] Daemon middleware buffers body, verifies headers, rejects missing/bad/stale signatures (401)
- [x] Signed control-plane client (`daemon.rs`) with matching secret
- [x] Manual E2E: unsigned, bad-signature, and stale daemon requests rejected

**Docker lifecycle**

- [x] bollard connect/ping, idempotent `mc-net` creation
- [x] Container create with `mc-<serverId>` name + `console.*` labels
- [x] itzg env translation: `EULA=TRUE`, `TYPE`, `MEMORY`, `SERVER_NAME`,
      `OVERRIDE_SERVER_PROPERTIES=false`, `ONLINE_MODE=false` for proxy backends
- [x] Absolute bind mount `<MC_DATA_ROOT>/<serverId>/data:/data`
- [x] Start / stop (35 s grace) / force-remove (data retained)
- [x] Strict limits: `Memory == MemorySwap`, `NanoCpus`, `PidsLimit=512`, `no-new-privileges`
- [x] Unit tests for env generation, limits, volume bind, proxy vs. standalone port exposure
- [x] Real Docker E2E: create → inspect → start → stop → delete

**Server CRUD (control plane)**

- [x] `GET /api/v1/servers`
- [x] `POST /api/v1/servers` with validation: name, type, port, `ram_mb`, `cpu_limit`,
      `is_behind_proxy` ⇒ `proxy_network_id` required; proxies cannot sit behind proxies
- [x] `409` on duplicate name/port (unique constraints), daemon-failure compensation deletes the row
- [x] `POST /api/v1/servers/{id}/start`, `/stop`
- [x] `DELETE /api/v1/servers/{id}`
- [x] Proxy rule on write: backends behind a proxy publish no host port
- [x] Unit tests for validation + rollback behaviour
- [x] `cargo check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo fmt` all clean

Deferred (not required for M2 sign-off): `GET /api/v1/servers/{id}`, transactional port allocator,
durable `jobs`, proxy-network CRUD, reconciliation loop, world-data deletion.

## M3 — Glassmorphic frontend & core UI ✅

**Theme + shell**

- [x] Aurora Glass Tailwind tokens (void/abyss/surface, neon accents, fonts, radii, glows)
- [x] `glass` / `glass-deep` utilities, `text-aurora`, rise animation, neon range slider
- [x] Local aurora placeholder artwork with dark `backdrop-blur-xl` overlay
- [x] Persistent sidebar (Dashboard, Servers, Settings) + responsive icon rail
- [x] Shared UI primitives: Button, GlassCard, StatusPill, TextField, banners, PageHeader

**Authentication**

- [x] Login page wired to `POST /api/auth/login`
- [x] Forced password-change screen for `403 password_change_required` / `/me` flag
- [x] JWT in `localStorage`, decoded for first paint, confirmed by `GET /api/v1/me`
- [x] Central fetch wrapper attaches Bearer; auth DOM events drive 401/403 screens
- [x] Sign-out clears the token and returns to login

**Dashboard + servers**

- [x] Dashboard stats (total/running, reserved RAM/CPU), recent servers, posture panel
- [x] Servers view fetches `/api/v1/servers` with loading/error/empty states
- [x] Glass server cards: status, type, RAM, CPU, port, proxy/network note
- [x] Start / stop / delete actions (two-step delete confirmation) with per-card busy state
- [x] Three-step creation wizard: runtime cards → RAM/CPU sliders → name/port/review
- [x] Client-side validation aligned with API rules; server errors surfaced inline
- [x] `npm run build` + `npm run lint` clean

**Verified**

- [x] Browser E2E: login → forced rotation → dashboard → create → start → stop → delete
- [x] No console errors during the flow

Delivered in M4: WebSocket telemetry, graphical console, `GET /api/v1/servers/{id}`.
Delivered in M6: sandboxed file I/O and the visual config editor. Still open:
proxy-network CRUD and status reconciliation.

## M4 — Real-time telemetry & graphical console ✅

**Daemon**

- [x] `bollard` `Docker::stats` → `StatsSample` (CPU delta, memory working set, network, PIDs)
- [x] `bollard` `Docker::logs(follow, timestamps)` → timestamped, ANSI-stripped `LogLine`s
- [x] HMAC-protected NDJSON streams at `/daemon/servers/{id}/stats` and `/logs`
- [x] Bounded channels + task cancellation when the control plane disconnects
- [x] Unit tests for CPU/memory/network maths and log frame splitting

**Control plane**

- [x] Signed streaming GET support in `DaemonClient::open_stream`
- [x] `GET /api/v1/ws/servers/{id}` WebSocket with `?token=` or `bearer.<jwt>` auth
- [x] Password-change gate + server-existence check before upgrade
- [x] Multiplex stats + logs into tagged JSON envelopes over one socket
- [x] `ready`, `stream_ended` and recoverable `error` frames; ping/pong handling
- [x] `GET /api/v1/servers/{id}` for refresh-safe detail views

**Frontend**

- [x] Clickable server cards → dedicated `ServerDetailPage`
- [x] `useServerTelemetry`: WS lifecycle, capped backoff, 600-line log ring buffer
- [x] Live stats panel: allocation-relative CPU ring, memory bar, RX/TX, PIDs
- [x] Graphical console: parsed time/source/level rows with colour-coded tones
- [x] Join/leave/chat detection, warning/error tones, filter chips, pause-on-scroll
- [x] Vite `/api` proxy upgrades WebSockets (`ws: true`)
- [x] `npm run build` + `npm run lint` clean

**Verified**

- [x] Real container: 5 stats frames + 119 log lines in 5 s over the browser socket
- [x] Playwright: login → servers → card click → streaming gauges → parsed console/filters
- [x] CPU ≈ 99.8%, memory ≈ 2.9 MiB reported from live Docker stats
- [x] No browser console errors

## M5 — Player manager

> Roadmap note: M5 is split. Part 1 (this milestone) ships the RCON foundation and the roster
> shell; authoritative vitals/inventory editing waits for the companion plugin (D2).

### Part 1 — RCON foundation & roster shell ✅

**Daemon**

- [x] Source RCON client: packet framing, auth, exec, timeouts, multi-packet reads
- [x] Credentials resolved from container env → `server.properties` → derived fallback
- [x] Per-server RCON password derived from the node secret; injected at container create
- [x] RCON published on `127.0.0.1:<derived 30000–39999>` only (Docker-Desktop safe)
- [x] HMAC-protected `POST /daemon/servers/{id}/command` with command validation
- [x] Unit tests: packet round-trip, properties parsing, auth failure, fake TCP RCON server

**Control plane**

- [x] `POST /api/v1/servers/{id}/command` for raw commands
- [x] `POST /api/v1/servers/{id}/players/{username}/kick|ban|op` semantic translation
- [x] Username/reason/command validation; injection attempts rejected with `400`
- [x] Running-server precondition; daemon 503 surfaced as `dependency_unavailable`

**Frontend**

- [x] `Players` tab on the server detail page
- [x] Responsive roster grid with Crafatar avatars, ping bars (live RCON names added in M6.5)
- [x] Hover Kick / Ban / Op actions wired to the API with optimistic roster updates
- [x] Slide-out inspection panel: disabled Vitals sliders + 9×4 inventory grid
- [x] "Requires Companion Plugin" badges/tooltips on both locked sections

**Verified**

- [x] `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` clean (**57 tests**)
- [x] `npm run build` + `npm run lint` clean
- [x] HTTP E2E against a fake RCON server: raw `list`, kick, ban, op all executed
- [x] Playwright: roster → inspection drawer → kick/ban/op notices, zero console errors
- [ ] Re-verify against a real Minecraft container (deferred to the manual E2E phase)

### Part 2 — Authoritative player state (companion plugin)

- [ ] NBT reader/writer for `playerdata/*.dat` (offline path)
- [ ] Offline edits: health, hunger, XP/level, gamemode, position, 9×4 inventory + armor
- [ ] Companion Paper plugin (separate Gradle project), in-thread edits, versioned protocol
- [ ] Plugin install/update flow; UI enables vitals/inventory when the plugin is present
- [ ] Roster join/leave events + UUID/name resolution (live `list` polling landed in M6.5)
- [ ] Whitelist / pardon / deop with audit entries
- [ ] Ranks/permissions bridge (LuckPerms, if ranked)
- [ ] Inventory grid drag/drop, item metadata, NBT tooltips

## M6 — Visual config editor ✅

> Scope for this milestone was the `server.properties` editor. Proxy CRUD, whitelist/ops/bans
> editors and durable jobs are deferred beyond the M6.5 testing gate.

**Daemon**

- [x] HMAC `GET/PUT /daemon/servers/{id}/files/{*path}`
- [x] Strict path sandbox: reject absolute paths, `..`, NUL, empty paths
- [x] Canonical root + per-component resolution; symlink escapes rejected
- [x] Reads limited to regular UTF-8 files ≤ 1 MiB
- [x] Atomic writes: sibling temp file → `fsync` → `rename`, cleanup on failure
- [x] Optional `expected_hash` → `409` on concurrent modification
- [x] Unit tests: traversal, nested paths, read/write round-trip, stale hash, symlink escape

**Control plane**

- [x] `GET /api/v1/servers/{id}/properties` → `{path, hash, entries, redacted}`
- [x] `PUT /api/v1/servers/{id}/properties {entries, expected_hash?}` → new document
- [x] Java properties parser preserving comments, blanks, order and unknown keys
- [x] Escapes/continuations: `\t \n \r \f \\ \uXXXX`, `=`/`:`/whitespace separators
- [x] Serialiser round-trips parsed entries; non-ASCII emitted as `\uXXXX`
- [x] `rcon.password` redacted in responses and restored from disk on save
- [x] Entry/key validation, 2000-entry cap, SHA-256 hash validation
- [x] Parser/redaction/hash unit tests

**Frontend**

- [x] `Configuration` tab on the server detail page
- [x] Schema metadata: sections, labels, hints, enum options, ranges, units, sliders
- [x] Glassmorphic toggles, enum dropdowns, range sliders, number/text inputs
- [x] Dirty/synced indicator, reload with discard confirmation, custom-key editor
- [x] `rcon.password` locked placeholder (value never reaches the browser)
- [x] Save uses the last hash; `409` surfaces a reload instruction
- [x] Restart-required modal after a successful save

**Verified**

- [x] `cargo fmt --check`, `cargo check`, `cargo clippy -D warnings`, `cargo test` clean (**71 tests**)
- [x] `npm run build` (52 modules) + `npm run lint` clean
- [x] API E2E: GET redaction, PUT round-trip, disk write, secret restoration, stale-hash `409`
- [x] Signed daemon traversal probes (`../../control.db`, `..%2F..%2Fcontrol.db`) rejected with `400`
- [x] Playwright: toggle PvP, edit MOTD, save, restart modal, reload persistence, zero console errors

**Deferred beyond M6.5**

- [ ] UI property search + diff preview
- [ ] Whitelist / ops / bans JSON editors
- [ ] Proxy files (`velocity.toml`, `forwarding.secret`, `config.yml`) and proxy-network CRUD
- [ ] Durable `jobs` table, async progress events, status reconciliation
- [ ] Multi-topic WS hub (status snapshots, sequence gaps) if needed

## M6.5 — Full manual end-to-end testing phase

> Required gate after M6 and before M7: exercise the product by hand with real Minecraft images.
> The first manual pass produced the fixes below. Retest is pending; later-milestone items stay
> unchecked.

**Patch set delivered**

- [x] Docker image auto-pull: daemon checks if image exists and pulls via `create_image` stream before container creation
- [x] Config numeric validation: skip blank optional numbers; accept `pause-when-empty-seconds=-1`
- [x] Centre glassmorphic toggle thumbs with flex alignment
- [x] Uniform configuration-card spacing (`flex flex-col gap-6` wrapper)
- [x] Migration `0003`: add `forge` to `server_type`, add `game_version` default `LATEST`
- [x] Forge runtime card in the create wizard
- [x] Version input in the wizard (free text + suggestions, defaults to `LATEST`)
- [x] API/daemon map `game_version` → itzg `VERSION` env
- [x] Daemon directory listing and safe regular-file deletion with traversal protection
- [x] API routes forwarding directory listing and file deletion
- [x] File Manager tab: table, folder navigation, breadcrumbs
- [x] Text-file editor for `.yml/.json/.properties/.txt` (plus related text formats)
- [x] Delete action with confirmation dialog

**Enhancement set delivered**

- [x] Daemon RCON `list` parser and `GET /daemon/servers/{id}/players`
- [x] `GET /api/v1/servers/{id}/players` returns online usernames
- [x] Players tab fetches real names and polls every 6 s while running
- [x] `POST /daemon/servers/{id}/restart` (graceful stop → start)
- [x] `POST /api/v1/servers/{id}/restart` with `stopping`/`running` status
- [x] Amber **Restart** button beside Start/Stop on the detail header
- [x] `PATCH /api/v1/servers/{id}/resources` persists `ram_mb`/`cpu_limit` with validation
- [x] Start/restart payloads carry current DB resources to the daemon
- [x] Daemon applies limits through Docker `update_container` before starting
- [x] Configuration tab **Resources & scaling** sliders + **Save Resources**
- [x] `?download=true` returns raw bytes with sanitised attachment headers
- [x] Raw binary upload through the daemon (`/raw-files`) and API (`PUT /files`)
- [x] `POST /directory/{path}` creates an empty folder
- [x] Raised Axum body limits to 100 MiB on raw upload routes
- [x] File Manager: Download, Upload File, New File, New Folder, Edit, Delete
- [x] Editor opens reliably from the row or the Edit action and refreshes the list after saving
- [x] `cargo fmt/check/clippy/test` clean (**79 tests**) and `npm run build/lint` clean

**Manual retest checklist**

- [ ] Config editor: toggle `online-mode`, save, reload, no numeric validation error
- [ ] Config editor edge cases: comments/blank lines, escaped values, custom keys, RCON password kept
- [ ] Create Forge server with a pinned version and with `LATEST`; verify `VERSION` in the container
- [ ] File Manager: navigate folders, edit and save a `.yml`/`.json`/`.properties`/`.txt` file
- [ ] File Manager: upload a binary (e.g. `.jar`), download it back, create a new file and folder
- [ ] File Manager: stale-hash conflict, delete confirmation, traversal rejection from the UI/API
- [ ] Players: roster reflects real `list` output; joins/leaves appear within one poll interval
- [ ] Players: kick/ban/op against a live server; verify roster/avatars and locked panels
- [ ] Restart: button halts and restarts the container; logs show a clean shutdown and boot
- [ ] Scaling: save larger RAM/CPU while stopped, start, confirm Docker HostConfig; repeat with restart
- [ ] Create, start, stop, restart and delete each server type
- [ ] Console: stdin commands, raw RCON endpoint, log parsing under load
- [ ] Auth edge cases, error states, responsive layout, accessibility pass
- [ ] Record findings; fix release blockers before M7
- [ ] Proxy routing/forwarding secrets (after proxy CRUD lands)
- [ ] Backup/restore + jobs/progress UI (deferred — after the App Store line of work)

## M6.7 — Core systems polish

> Pre-App-Store polish driven by manual testing. Delivered before M7; review pending.

**Offline roster**

- [x] Daemon reads/parses `usercache.json` (`name`, `uuid`, `expiresOn`)
- [x] `GET /daemon/servers/{id}/players/history` and `GET /api/v1/servers/{id}/players/history`
- [x] Players tab merges live RCON names with cached names
- [x] Green **Online** / gray **Offline** indicators on every roster card

**Player Inspector**

- [x] Removed the disabled Health/Hunger/Inventory placeholder UI
- [x] Daemon reads `ops.json`, `whitelist.json` and `banned-players.json`
- [x] Access toggles use RCON while running and edit JSON files while stopped
- [x] `GET /api/v1/servers/{id}/players/access` and `PUT …/players/{username}/access`
- [x] Inspector toggles: Operator, Whitelist, Ban (work online and offline)
- [x] Roster cards expose ban/unban and op/deop; kick remains online-only
- [x] Clear `409` when an offline add has no UUID in `usercache.json`

**File Manager**

- [x] `POST /daemon/servers/{id}/files/rename` (`old_path`, `new_path`)
- [x] `POST /daemon/servers/{id}/files/copy` (`source_path`, `dest_path`)
- [x] API forwards both operations under `/api/v1/servers/{id}/files/{rename,copy}`
- [x] Strict canonicalisation and traversal/symlink checks on both source and destination
- [x] Rename action on every file/folder row
- [x] Copy and Move options in a row action menu; Move reuses the rename endpoint
- [x] Clickable Name/Size columns with ascending/descending sort
- [x] `cargo fmt/check/clippy/test` clean (**83 tests**) and `npm run build/lint` clean

**Review checklist**

- [ ] Stop a server and confirm previously joined players appear offline in the Players tab
- [ ] Toggle op/whitelist/ban for an offline player and inspect the JSON files on disk
- [ ] Toggle the same while running and confirm RCON applies immediately
- [ ] Rename a file, copy a folder, move a file to another folder
- [ ] Attempt traversal (`../`) and an existing destination; confirm rejection
- [ ] Sort by Name and Size in both directions

## M8 — Advanced App Store (Modrinth)

> Integrated Modrinth browser for plugins, mods, datapacks and resource packs. Modpacks/.mrpack is
> explicitly deferred to M8.5. Delivered; polish landed in M8.1.

**Backend**

- [x] `POST /daemon/servers/:id/downloads` with `{url, filename, target_directory}`
- [x] HTTPS-only source URLs; loopback/private hosts, credential URLs and redirects rejected
- [x] Streamed download to a temp file + fsync + atomic rename
- [x] 50 MiB cap enforced from `Content-Length` and while streaming
- [x] `target_directory` validated component-by-component and canonicalised against the server root
- [x] Missing directories (e.g. `mods/`, `<world>/datapacks/`) created safely
- [x] `POST /api/v1/servers/:id/addons/install` accepts a Modrinth version URL + `project_type`
- [x] Modrinth host + jar/zip extension validation at the API boundary
- [x] Mods → `mods/` (fabric/forge only); Plugins → `plugins/` (paper/velocity/bungeecord only)
- [x] Datapacks → `<level-name>/datapacks/`, with `level-name` read from `server.properties`
- [x] Resource packs → `plugins/Geyser-Spigot/packs/` when Geyser exists, else `resourcepacks/`
- [x] `GET /api/v1/servers/:id/addons` aggregates `.jar`/`.zip` from the dynamic directories

**Frontend**

- [x] **App Store** tab on the Server Detail page
- [x] Modrinth search against `api.modrinth.com/v2/search`
- [x] Project-type dropdown (Mod / Plugin / Datapack / Resource Pack)
- [x] Dynamic category facets from the server type (`categories:fabric`, `categories:paper`, …)
- [x] Result cards with icon, title, author, downloads and categories
- [x] Install flow resolves the latest compatible version file and posts it to `/install`
- [x] Per-project loading spinner and success/error banners
- [x] Installed list with folder, size, type badge and confirmed Delete
- [x] Notice to restart the server after installing
- [x] `cargo fmt/check/clippy/test` clean (**89 tests**) and `npm run build/lint` clean

**Review checklist**

- [ ] Search each project type on a matching server (fabric/forge, paper/velocity/bungee, vanilla)
- [ ] Install a mod, a plugin, a datapack and a resource pack; verify the destination paths on disk
- [ ] Confirm datapacks land under the configured `level-name` after editing `server.properties`
- [ ] With Geyser installed, confirm resource packs land in `plugins/Geyser-Spigot/packs/`
- [ ] Reject an over-50 MiB file and a non-Modrinth / `http` URL from the API
- [ ] Restart the server and confirm the installed jar/zip is loaded
- [ ] Delete an installed add-on and confirm it disappears from disk and the list
- [ ] Confirm an unsupported combination (mod on Paper, plugin on Fabric) is rejected

## M8.1 — App Store polish

> UX pass after M8 manual testing. Frontend-only; the M8 backend contract is unchanged. Review
> pending.

**Strict contextual filtering**

- [x] Project-type dropdown only offers kinds the server can load
- [x] Fabric/Forge hide Plugins and default to Mods
- [x] Paper/Velocity/BungeeCord hide Mods and default to Plugins
- [x] Datapacks and Resource Packs remain available on every server kind
- [x] Search facets follow the selected kind and server loader

**Rich details & version selector**

- [x] Clicking a card opens a project-details modal
- [x] Full Modrinth `body` rendered as GitHub-flavoured Markdown (`react-markdown` + `remark-gfm`)
- [x] Sanitised raw HTML subset (`rehype-raw` + `rehype-sanitize`) for Modrinth `<center>`, `<img>`,
      `<br>` and YouTube/Vimeo `<iframe>` embeds; untrusted hosts and non-HTTPS sources are dropped
- [x] Versions filtered by loader and the server's exact `game_version`
- [x] Latest compatible release preselected; older compatible versions selectable
- [x] Install sends the selected version file URL to `/addons/install`
- [x] `LATEST`/`SNAPSHOT` server versions skip the game-version filter
- [x] Modal closes on Escape/backdrop and links back to the Modrinth project page

**Reactive install state**

- [x] Search cards cross-reference `GET /api/v1/servers/:id/addons`
- [x] Installed projects show a red **Delete** instead of **Install**
- [x] Delete uses the file endpoint and reverts the card to **Install**
- [x] Per-server project → filename map keeps matching stable across refreshes
- [x] Quick-install from a card still installs the latest compatible version

**Installed view**

- [x] Always-visible **Discover / Installed** sub-tabs in the App Store
- [x] Installed files grouped by folder (`plugins/`, `mods/`, `<world>/datapacks/`, …)
- [x] Folder headers show file counts; rows show name, size, type and Delete
- [x] Empty and loading states for the installed list
- [x] `cargo check`, `cargo test` (89 tests) and `npm run build`/`npm run lint` clean

**Review checklist**

- [ ] Paper server: confirm Mods is absent, Plugins is selected, Datapacks/Resource Packs remain
- [ ] Fabric server: confirm Plugins is absent, Mods is selected, Datapacks/Resource Packs remain
- [ ] Open a project, confirm the Markdown body renders (headings, lists, links, images, tables)
- [ ] Open WorldEdit and confirm raw HTML renders as centred images/YouTube embeds, not literal tags
- [ ] For a server pinned to e.g. `1.21.1`, confirm only `1.21.1`-tagged versions are listed
- [ ] Install an older compatible version and verify that exact file lands on disk
- [ ] Install from a card, confirm it flips to Delete; delete it and confirm it flips back
- [ ] Refresh/reopen the App Store and confirm installed cards are still marked Delete
- [ ] Switch to Installed and confirm grouping, counts, sizes and per-file Delete
- [ ] Install for each of mods, plugins, datapacks and resource packs and restart to load

## M8.5 — Modpack engine (`.mrpack`) & version selector polish

> Adds Modrinth modpack installs and replaces the flat version list with bounded Loader / Game
> Version dropdowns for every project type. Delivered; review pending.

**Daemon modpack engine**

- [x] `POST /daemon/servers/:id/modpacks/install` accepting `{url}` to a `.mrpack`
- [x] Stream the archive to a temp file inside the volume (200 MiB cap) and always clean it up
- [x] Parse `modrinth.index.json` in a blocking worker (4 MiB index cap)
- [x] Skip files whose `env.server` is `unsupported`
- [x] Re-validate every pack-file URL (HTTPS only, public hosts, no redirects, no credentials)
- [x] Concurrent downloads through `tokio::task::JoinSet` (limit 12), 100 MiB per file
- [x] Atomic temp-file writes and refusal to replace symlinks/directories
- [x] Flatten index paths to unique basenames under `mods/`; reject duplicates and traversal
- [x] Extract `overrides/` into in-volume staging, then apply over the server root (overwrites files)
- [x] Override safety: traversal/symlink rejection, 100 MiB per file, 512 MiB total, 20k entries
- [x] Clean up the staging directory and temp archive on success or failure
- [x] Unit tests for filtering, traversal/duplicate rejection, HTTPS enforcement and override extraction

**API**

- [x] `AddonProjectType::Modpack` with `.mrpack` extension validation
- [x] `POST /api/v1/servers/:id/addons/install` forwards `project_type=modpack` to the daemon engine
- [x] Modpacks restricted to Fabric/Forge; clear rejection on other server kinds
- [x] Daemon client gained a 900-second per-request timeout for modpack installs
- [x] Shared `ModpackInstallRequest` / `ModpackInstallResult` types

**Frontend**

- [x] **Modpack** appears in the project-type filter only for Fabric/Forge
- [x] Modpack search facets use the server loader category
- [x] Modpack installs select the `.mrpack` file
- [x] Persistent **Installing Modpack (This will take a few minutes)…** banner with spinner
- [x] Details modal uses **Loader / Software** + **Game Version** dropdowns for every project type
- [x] Loader options intersect project loaders with what the server can run
- [x] Game-version options derive from file-bearing versions for the chosen loader, deduplicated
- [x] Dropdowns default to the server's loader and exact `game_version`
- [x] Changing either dropdown re-derives versions and resolves the matching Modrinth release
- [x] Datapacks/resource packs use a synthetic `minecraft` loader
- [x] `cargo fmt/check/clippy/test` clean (**94 tests**) and `npm run build/lint` clean

**Review checklist**

- [ ] On a Fabric and a Forge server, confirm **Modpacks** appears; confirm it is hidden elsewhere
- [ ] Install a real `.mrpack` and confirm the long-running banner shows for the duration
- [ ] Verify pack files land in `mods/` and `env.server = "unsupported"` files are absent
- [ ] Confirm `overrides/` files are applied to the server root and overwrite existing configs
- [ ] Confirm the temp `.mrpack` and staging directory are gone after success and after a failure
- [ ] Reject a non-`.mrpack`, non-Modrinth or `http` URL and a modpack attempt on Paper
- [ ] Open a mod/plugin/datapack/resource pack and confirm both dropdowns reflect real support only
- [ ] For a server pinned to e.g. `1.21.1`, confirm the game-version dropdown defaults to `1.21.1`
- [ ] Change each dropdown and confirm the resolved release filename updates and installs correctly

## M8.8 — Migration engine & production prep

> Delivered; automated checks pass. Manual import review is pending. M8.9 subsequently delivered
> backups, scheduling, App Store fixes and the MCP server; M10 (multi-node) remains deferred.

**Migration engine**

- [x] `POST /daemon/servers/import` supports a streamed ZIP upload or absolute daemon-host path
- [x] Route-specific 10 GiB request limit; ZIP is streamed to disk before blocking extraction
- [x] ZIP extraction and local recursive copy reject traversal, symlinks, duplicate paths and
  non-regular files; failed imports remove partial data
- [x] `POST /api/v1/servers/import` accepts server name/type/version/RAM/CPU plus one source
- [x] API allocates a game port, provisions the stopped container, streams data to the daemon,
  registers the row and rolls back on import failure
- [x] Dashboard and Servers page expose the three-step Import Server wizard

**Production deployment**

- [x] `deployment/nginx.conf`: port 80, React static root, `/api/` proxy, WebSocket upgrades,
  Basic auth and large-upload streaming
- [x] `deployment/cluster-api.service` and `deployment/cluster-daemon.service`
- [x] `deployment/README.md`: build/install, environment, `.htpasswd`, Nginx and systemd setup

**Verified**

- [x] `cargo check`
- [x] `cargo test` — 103 passed (36 API, 53 daemon, 14 shared)
- [x] `npm run build` — 306 modules; Vite reports the existing >500 kB chunk warning

**Manual review checklist**

- [ ] Import a ZIP with root-level server files and start the imported server
- [ ] Import a ZIP with one wrapper directory and confirm only that wrapper is stripped
- [ ] Import from an absolute host path; confirm source remains unchanged
- [ ] Reject traversal/symlink ZIP entries and confirm failed imports leave no partial server row
- [ ] Confirm both dashboard and Servers entry points open the wizard and validation works
- [ ] Verify Nginx auth, API routing, WebSocket upgrade and a large upload on the VPS

## M8.9 — Automation, App Store fixes & MCP (markdown context protocol)

> Delivered; automated checks pass. Manual review is pending. This work was requested after M8.8
> and does not start M10 (multi-node).

**Create & App Store fixes**

- [x] Runtime category buttons select a card in that category; card selection updates the category
- [x] Validate `world_name` (single safe path component) and `world_seed` on the API
- [x] Enumerate `world_type`, `difficulty` and `gamemode` on both client and server
- [x] Import the wizard's optional data ZIP through `POST /api/v1/servers/{id}/import-data`
- [x] Keep the created server and show an "Open server" fallback when the import fails
- [x] `ServerType::supports_plugins/supports_mods/supports_modpacks` for all 19 runtimes
- [x] App Store destination + loader filtering uses the capability helpers (NeoForge, Quilt,
      Spigot, Purpur, Pufferfish, Folia, Sponge, hybrids, Waterfall)

**Backups**

- [x] Daemon ZIP engine: whole `/data`, deflate, symlink-safe, size/entry caps
- [x] SHA-256 archive hash + JSON sidecar manifest; list ignores corrupt entries
- [x] Backup a running server with `save-off` → `save-all flush` → `save-on` best effort
- [x] Verified restore: hash check → staging extraction → atomic data-directory swap → restart
- [x] Delete archive + manifest; API catalog table (`0005`)
- [x] API routes: list/create/delete/restore
- [x] Automation tab: create, restore with confirmation, delete

**Scheduled tasks**

- [x] `scheduled_tasks` table: interval, enabled, next/last run, last status/error
- [x] Task types: `backup`, `start`, `stop`, `restart`, `command`
- [x] 20 s in-process worker with claim-before-run semantics
- [x] API CRUD + run-now with validation
- [x] Automation tab: create, pause/enable, run now, delete

**MCP server**

- [x] `mcp/` Node package using `@modelcontextprotocol/sdk` over stdio
- [x] Every repository markdown file exposed as a `cluster://docs/<path>` resource
- [x] Tools for docs, servers, backups, schedules and runtime capabilities
- [x] JWT auth, lazy username/password login, one transparent re-login after 401
- [x] Destructive annotations; restore creates a safety backup by default
- [x] `mcp/README.md` with client configs and security notes
- [x] `npm test` — 6 tests (docs, URI, auth, errors, network)

**Verified**

- [x] `cargo check --workspace --all-targets`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace` — 110 passed (40 API, 56 daemon, 14 shared)
- [x] `npm run build` — 306 modules; `npm run lint` clean
- [x] `cd mcp && npm test` — 6 passed

**Manual review checklist**

- [ ] Create one server per runtime category and confirm the selected card/category/review agree
- [ ] Create with a data ZIP; confirm the archive is imported into the stopped container
- [ ] Install a plugin on Spigot/Purpur/Folia and a mod on NeoForge/Quilt
- [ ] Back up a running server, mutate a file, restore, confirm revert + restart
- [ ] Verify a healthy backup (badge shows verified), download it, and confirm the ZIP opens
- [ ] Set retention to 2, create three backups, confirm only the two newest remain
- [ ] Scheduled backup produces a catalog row and archive; command task records success/error
- [ ] MCP client lists resources, reads `docs/state.md` and runs a backup tool

## M8.9.1 — Runtime compatibility & telemetry cleanup

> Delivered; automated checks pass. Manual review is pending.

- [x] `GET /api/v1/runtimes/{server_type}/versions` backed by each runtime's release index
      (Mojang, Arclight GitHub, CatServer GitHub, Mohist API, Magma API)
- [x] Create/import wizards load runtime-specific versions and default pinned runtimes to a real build
- [x] Reject `LATEST`/`SNAPSHOT` for Arclight/CatServer/Mohist/Magma on the API
- [x] Arclight `ARCLIGHT_TYPE=NEOFORGE` + daemon-pinned `ARCLIGHT_RELEASE`; CatServer via
      `TYPE=CUSTOM` + `CUSTOM_SERVER`
- [x] Resolve `MAGMA_VERSION` from the Magma API for a concrete Minecraft version
- [x] Velocity/BungeeCord/Waterfall use `MC_PROXY_IMAGE` and mount `/server`; BungeeCord/Waterfall
      publish `25577/tcp`, Velocity `25565/tcp`
- [x] Remove the daemon client's total timeout from long-lived stats/log streams
- [x] Treat missing optional folders as empty listings; log daemon `404` at debug
- [x] Map early RCON `Connection refused` to retryable `409 Conflict`
- [x] `cargo check`, `clippy -D warnings`, `cargo test` — 110 passed (40 API, 56 daemon, 14 shared)
- [x] `npm run build` — 306 modules; `npm run lint` clean

**Manual review**

- [ ] Create Arclight with a version from the dropdown; confirm the container reaches RCON/console
- [ ] Repeat for CatServer, Mohist and Magma; confirm their resolved installer env is used
- [ ] Start a proxy and connect through it on the published host port
- [ ] Leave a server detail page open for several minutes; confirm stats/log streams stay connected
- [ ] Open a fresh server before `server.properties` exists; confirm only an empty-state message and
      no warning/error log spam
- [ ] Click a console action during startup; confirm the "still starting" message is retryable

## M7 — Hardening & ops

- [ ] RBAC (owner/admin/operator/viewer) on every route + WS topic
- [ ] Audit log UI
- [ ] Rate limiting, CSRF stance, secure cookies, self-service password reset, session revocation
- [ ] Docker hardening: `no-new-privileges`, read-only root where feasible, PIDs limit, disk quota
- [ ] Panic recovery, timeouts, graceful shutdown both binaries
- [ ] `tracing` spans API → daemon, `/metrics`, health dashboards
- [ ] Dockerfiles (api, daemon, web), compose stack, Caddy config, systemd unit
- [ ] Rust tests: port allocator, properties round-trip, NBT round-trip, RBAC (HMAC auth ✅ M2)

## M9 — Backup center — **partially delivered in M8.9**

- [x] Snapshot: `save-off` → `save-all flush` → ZIP `/data` → `save-on` (best effort)
- [x] SHA-256 manifest + catalog row (`backups`); no progress events yet
- [x] Retention policy: per-server keep-last-N (`backup_settings`, migration `0006`); pruning
      runs after each manual or scheduled backup and `0` keeps everything
- [x] Restore: verify hash → staged extraction/traversal checks → directory swap → restart
      (safety backup is automatic only in the MCP tool)
- [x] Download archives: streamed through the signed daemon link, 64 KiB chunks
- [x] Integrity verify endpoint + per-row verified/corrupt badge in the Automation tab
- [ ] Upload archives
- [ ] Retention buckets (hourly/daily/weekly) instead of keep-last-N
- [ ] UI: full timeline and live create progress (list + restore confirmation exist)
- [ ] Proxy-aware backups (world vs. proxy config-only snapshots)
- [ ] Locked backups (exclude from retention)
- [ ] Off-host sync / notifications

## M10 — Multi-node & polish

- [ ] `nodes` table + `servers.node_id` migration, node enrollment UI, capacity dashboard
- [ ] Daemon over TLS + mTLS; node health/timeout handling
- [ ] Live node metrics (CPU/RAM/disk) charts
- [ ] Server templates, clone, import existing world
- [x] Scheduled backups + restarts (M8.9; no in-game warning yet)
- [ ] In-game warnings before scheduled restarts/backups
- [ ] Player session history / playtime analytics
- [ ] i18n, accessibility pass, reduced-motion mode, mobile layout
- [ ] Optional PostgreSQL backend behind the same store interface
