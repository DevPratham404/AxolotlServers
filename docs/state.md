# State — Cluster Console

**Last updated:** 2026-11-15
**Phase:** **M8.8 Migration engine & production prep delivered** — existing servers can be imported
from a ZIP upload or an absolute host path, and `deployment/` contains the Nginx, systemd and
operator files for an Oracle VPS. Next: manual review, then wait for approval before M9. Do not
start M9 or backups.

## 1. TL;DR

- Backend is clean: `cargo fmt --check`, `cargo check --workspace --all-targets`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test` (**100 tests**:
  35 API + 51 daemon + 14 shared) all pass.
- Frontend is clean: `npm run build` (tsc + Vite, 305 modules) and `npm run lint` (oxlint) pass.
- **Import engine (M8.8):** `POST /daemon/servers/import` accepts multipart with exactly one source:
  a streamed `file` ZIP (10 GiB body limit) or an absolute `local_path` on the daemon host. ZIPs are
  streamed to `<MC_DATA_ROOT>/.imports`, then extracted in a blocking worker; local paths are
  validated and recursively copied. Both paths land in `<MC_DATA_ROOT>/<uuid>/data`, reject
  traversal/symlinks/duplicates/non-regular files, enforce per-file and total caps, and clear
  partial data on failure. A single wrapper folder is stripped only when it contains server markers.
- **API import (M8.8):** `POST /api/v1/servers/import` accepts multipart `name`, `server_type`,
  `game_version`, `ram_mb`, `cpu_limit` plus `file` **or** `local_path`. It auto-allocates a game
  port, inserts the row, creates the stopped container **before** consuming a large upload, streams
  the source to the daemon under digest-based HMAC (`x-cluster-body-sha256`), and rolls back the
  container and row if the import fails.
- **Import wizard (M8.8):** Dashboard and Servers now have an **Import Server** button beside
  **New Server**. The three-step wizard covers Source (ZIP upload or local path), Configuration
  (name, type, version, RAM, CPU), and Import, with the persistent
  **Importing and extracting server (this may take a while)…** loading banner.
- **Production deployment (M8.8, files only):** `deployment/nginx.conf` terminates HTTP on port 80,
  serves `/var/www/cluster-panel`, proxies `/api/` to `127.0.0.1:3000` with WebSocket upgrades,
  streams large uploads without buffering, and enforces `auth_basic`. `cluster-api.service` and
  `cluster-daemon.service` run the binaries under systemd, and `deployment/README.md` covers build,
  `.htpasswd`, Nginx, environment files and enablement.
- **Modpacks (M8.5):** `POST /daemon/servers/{id}/modpacks/install` streams a `.mrpack`, parses
  `modrinth.index.json`, skips `env.server = "unsupported"` entries, downloads the rest into `mods/`
  with 12-way concurrency, then applies `overrides/` over the server root. The API forwards
  `project_type: modpack` to this engine and only allows it on Fabric/Forge.
- **Version selectors (M8.5):** the details modal replaces the flat version list with **Loader /
  Software** and **Game Version** dropdowns for every project type. Options are derived only from
  versions that have a matching file and support the selected loader; the pair resolves the exact
  Modrinth release. Defaults follow the server's loader and `game_version`.
- **App Store (M8.1):** the Server Detail page has an **App Store** tab with **Discover** and
  **Installed** sub-tabs. Discover is strictly bound to the server kind: mods are unavailable on
  Paper/Velocity/BungeeCord, plugins are unavailable on Fabric/Forge, and datapacks/resource packs
  are always offered (plus modpacks on Fabric/Forge). Clicking a card opens a details modal with the
  full Modrinth Markdown body and Loader / Game Version dropdowns; the matching release is resolved
  from the chosen pair. Installed projects show a red **Delete** in place of **Install**.
- **Daemon downloader:** `POST /daemon/servers/{id}/downloads` streams an HTTPS file into a
  validated `target_directory` with a 50 MiB cap, temporary-file write and atomic rename. Public
  hosts only; loopback/private literals and `.local`/`.internal` names are rejected, and redirects
  are disabled.
- **Dynamic destinations:** mods → `mods/` (Fabric/Forge only), plugins → `plugins/`
  (Paper/Velocity/BungeeCord only), datapacks → `<level-name>/datapacks/` (read from
  `server.properties`, default `world`), resource packs → `plugins/Geyser-Spigot/packs/` when
  Geyser is installed, otherwise `resourcepacks/`.
- **Installed list:** `GET /api/v1/servers/{id}/addons` aggregates `.jar`/`.zip` files from the same
  dynamic directories. The Installed sub-tab groups them by folder (`plugins/`, `mods/`,
  `<world>/datapacks/`, …) with per-file Delete buttons, and card state stays reactive after an
  install or delete.
- The daemon now exposes HMAC-protected `GET/PUT/DELETE /daemon/servers/{id}/files/{*path}` plus
  `GET/POST /daemon/servers/{id}/directory[/{*path}]`, `POST …/files/rename`, `POST …/files/copy`,
  all using the same strict path sandbox, canonical/symlink escape checks, 1 MiB text cap and a
  100 MiB raw upload/download cap.
- **Offline roster:** `GET /api/v1/servers/{id}/players/history` parses `usercache.json`; the
  Players tab merges it with the live RCON `list`, showing green **Online** and gray **Offline**
  dots. History is available while the server is stopped.
- **Player access controls:** `GET /api/v1/servers/{id}/players/access` reads `ops.json`,
  `whitelist.json` and `banned-players.json`; `PUT …/players/{username}/access` toggles operator,
  whitelist and ban. A running server is driven by RCON; a stopped server edits the JSON files
  directly. The Player Inspector now ships these controls instead of the fake vitals/inventory.
- **File operations:** `POST /api/v1/servers/{id}/files/rename` and `…/files/copy` accept
  `old_path`/`new_path` and `source_path`/`dest_path`. Both source and destination pass the full
  traversal + symlink sandbox. The UI adds Rename, a Copy/Move menu, and sortable Name/Size columns.
- **Live player roster:** `GET /api/v1/servers/{id}/players` runs RCON `list`, parses usernames and
  the Players tab polls it every 6 seconds while the server is running.
- **Restart:** `POST /api/v1/servers/{id}/restart` stops the container gracefully (35 s timeout)
  and starts it again; the detail header has an amber Restart button next to Start/Stop.
- **Dynamic scaling:** `PATCH /api/v1/servers/{id}/resources` updates SQLite, and every
  start/restart carries the latest `ram_mb`/`cpu_limit` to the daemon, which calls Docker
  `update_container` before starting. The Configuration tab has wizard-style RAM/CPU sliders and
  **Save Resources**.
- **Full file manager:** authenticated download (`?download=true`), binary upload, new file, new
  folder, reliable text editor, rename/copy/move, sorting, and refresh after every mutation.
- The daemon automatically verifies and pulls the required Docker image (`ensure_image` via
  `inspect_image` and `create_image` stream consumption) before attempting container creation.
- The API exposes `GET/PUT /api/v1/servers/{id}/properties` and forwards the directory/file routes.
  It parses the Java properties format into ordered entries, redacts `rcon.password`, restores it
  on save, and uses the SHA-256 hash for optimistic concurrency.
- The server detail page has **Console**, **Players**, **Configuration**, **File Manager** and
  **App Store** tabs.
- Last commit: `99135af feat: M8 advanced app store with strict contextual filtering, markdown details, and reactive install states`.
  **M8.5 and M8.8 changes are uncommitted.**
- Roadmap gate: **M8.8 review must pass before M9 (backups).** Backups and multi-node remain
  deferred. Do not start M9, backups or the companion plugin yet.

## 2. Phase status

| Area | Status |
| --- | --- |
| Control plane auth + forced rotation | ✅ M2 |
| Database schema | ✅ M2 |
| Server CRUD endpoints | ✅ M2 (detail `GET` added in M4) |
| Daemon control link / Docker lifecycle / HMAC | ✅ M2 |
| Aurora Glass frontend, auth, dashboard, wizard | ✅ M3 |
| Daemon stats/log NDJSON streams | ✅ M4 |
| `/api/v1/ws/servers/{id}` multiplexer + WS JWT | ✅ M4 |
| Server detail page (live stats + console) | ✅ M4 |
| RCON client + credential resolution | ✅ M5-Part 1 |
| Raw command API + semantic kick/ban/op | ✅ M5-Part 1 |
| Players tab + inspection scaffold | ✅ M5-Part 1 |
| Live RCON player roster + 6 s polling | ✅ M6.5 |
| Offline roster from `usercache.json` + online/offline indicators | ✅ M6.7 |
| Player Inspector access toggles (op/whitelist/ban, online or offline) | ✅ M6.7 |
| Graceful restart endpoint + UI button | ✅ M6.5 |
| Dynamic RAM/CPU scaling from the Configuration tab | ✅ M6.5 |
| Sandboxed daemon file read/write | ✅ M6 |
| Properties parser + redaction + concurrency | ✅ M6 |
| Configuration tab + dynamic form + restart modal | ✅ M6 |
| Config numeric validation + toggle/spacing fixes | ✅ M6.5 |
| Forge runtime + `game_version` → `VERSION` env | ✅ M6.5 |
| File Manager (list/navigate/edit/delete) | ✅ M6.5 |
| File Manager upload/download/new file/new folder | ✅ M6.5 |
| File Manager rename/copy/move + sortable columns | ✅ M6.7 |
| Modrinth App Store discovery + install + installed list | ✅ M8 |
| Streamed daemon downloader + 50 MiB cap | ✅ M8 |
| Strict server-type filtering + Markdown details + version picker | ✅ M8.1 |
| Reactive Install/Delete state + grouped Installed view | ✅ M8.1 |
| Modpack engine (`.mrpack` download, concurrent files, overrides) | ✅ M8.5 |
| Loader + game-version dropdowns for every project type | ✅ M8.5 |
| Server import engine (ZIP stream/local path, sandboxed extraction) | ✅ M8.8 |
| API import + auto port + rollback + import wizard | ✅ M8.8 |
| Production Nginx / systemd / deployment guide | ✅ M8.8 |
| Live vitals/inventory (companion plugin) | ❌ removed from UI in M6.7; remains future plugin work |
| Proxy-network CRUD, durable jobs | ❌ deferred beyond M6.5 |
| Manual end-to-end testing gate | ⏭ **M8.8 — review pending** |
| Hardening & ops | ❌ M7 (deferred until after the store work) |
| Backups | ❌ deferred |
| Multi-node | ❌ deferred |

## 3. Verified working

### Backend

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ clean |
| `cargo check --workspace --all-targets` | ✅ clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ clean |
| `cargo test --workspace` | ✅ 100 passed (35 API + 51 daemon + 14 shared) |

New M6 unit coverage: traversal/absolute/NUL path rejection, nested path normalisation, read/write
round-trip, stale-hash `409`, Unix symlink escape rejection, Java escape/continuation parsing,
comment/blank round-trip, `:` and whitespace separators, `\uXXXX`, redaction/restoration, entry
validation, hash validation and daemon path percent-encoding.

New M6.5 coverage: directory listing (root + nested, dirs first), file deletion, directory-delete
rejection, traversal rejection on delete, game-version normalisation, signed directory path
construction, RCON `list` parsing, scaling-limit validation, Docker `ContainerUpdateBody`
construction, and download-filename sanitisation.

New M6.7 coverage: rename/copy round-trips (files and directories), destination traversal rejection,
copy-overwrite conflict, `usercache.json` parsing, offline op/whitelist/ban writes (including the
expected `ops.json` level and `banned-players.json` expiry), access revocation, UUID-missing conflict,
plus router-dispatch tests for the new history/access/rename/copy routes.

New M8 coverage: URL validation (HTTPS only, loopback/private/credential URLs rejected), filename
validation, nested download-directory creation (including traversal rejection), per-server-type
destination mapping, dynamic datapack/resource-pack directories, `level-name` validation, Modrinth
host/extension validation, and router-dispatch tests for the add-on endpoints.

New M8.5 coverage: `.mrpack` index parsing with `env.server = "unsupported"` filtering, duplicate
and traversal pack-path rejection, HTTPS enforcement for every pack-file URL, `.mrpack` filename
validation, override extraction that ignores non-`overrides/` entries, and API destination mapping
for modpacks (Fabric/Forge only).

New M8.8 coverage: digest-based HMAC signing/verification for streamed bodies (body, method and path
tampering rejected), ZIP path traversal/absolute-path rejection, backslash normalisation,
single-wrapper-folder detection, bare-world and root-file wrapper refusals.

### Frontend

| Check | Result |
| --- | --- |
| `npm run build` (`tsc -b && vite build`) | ✅ 305 modules, no TS errors |
| `npm run lint` (`oxlint`) | ✅ clean |
| Bundle | ✅ ~565 kB JS / 72 kB CSS (162 kB / 11 kB gzip); Vite still warns about the >500 kB chunk |

### End-to-end config editor

Executed against a real daemon + API (temp SQLite DB and temp `MC_DATA_ROOT`) with a seeded
`server.properties`:

| Check | Result |
| --- | --- |
| `GET /api/v1/servers/{id}/properties` | ✅ ordered entries, comments/blanks preserved |
| `rcon.password` in GET response | ✅ value empty, `redacted: ["rcon.password"]` |
| `PUT` changes `motd` + `pvp` | ✅ 200, new hash, disk content updated |
| `rcon.password` after save | ✅ original `super-secret-rcon` restored on disk |
| Stale `expected_hash` on PUT | ✅ `409 conflict` |
| Daemon signed `GET …/files/../../control.db` | ✅ `400` — `path must not contain '..'` |
| Daemon signed `GET …/files/..%2F..%2Fcontrol.db` | ✅ `400` — encoded traversal rejected |
| Playwright: Configuration tab, toggle PvP, edit MOTD, save | ✅ |
| Restart-required modal | ✅ shown after save; dismiss works |
| Reload + reopen Configuration | ✅ `motd` and `pvp` persisted |
| Browser console errors | ✅ none |

The file endpoints do not require Docker or a running container; they operate on the bind-mounted
server data directory. A real Minecraft start/restart check remains part of M6.5.

## 4. API surface (implemented)

| Method | Path | Auth | Behaviour |
| --- | --- | --- | --- |
| GET | `/healthz` | none | liveness |
| GET | `/readyz` | none | SQLite check; 503 when unavailable |
| POST | `/api/auth/login` | none | `{username, password}` → token + user |
| POST | `/api/auth/change-password` | Bearer (pending allowed) | rotates hash, clears flag, returns fresh token |
| GET | `/api/v1/me` | Bearer | echoes `{id, username, requires_password_change}` |
| GET | `/api/v1/servers` | Bearer | list, newest first |
| GET | `/api/v1/servers/{id}` | Bearer | single server (refresh-safe detail view) |
| POST | `/api/v1/servers` | Bearer | create + provision via daemon → 201 |
| POST | `/api/v1/servers/import` | Bearer | multipart config + `file` **or** `local_path` → container + imported volume → 201 |
| POST | `/api/v1/servers/{id}/start` | Bearer | daemon start with current DB resources → `status=running` |
| POST | `/api/v1/servers/{id}/stop` | Bearer | daemon stop → `status=stopped` |
| POST | `/api/v1/servers/{id}/restart` | Bearer | daemon stop → apply resources → start → `status=running` |
| PATCH | `/api/v1/servers/{id}/resources` | Bearer | `{ram_mb?, cpu_limit?}` → updated server |
| DELETE | `/api/v1/servers/{id}` | Bearer | daemon remove → row deleted → 204 |
| GET | `/api/v1/servers/{id}/players` | Bearer | RCON `list` → `["Steve", …]` |
| GET | `/api/v1/servers/{id}/players/history` | Bearer | `usercache.json` → `[{name, uuid, expires_on}]` (works stopped) |
| GET | `/api/v1/servers/{id}/players/access` | Bearer | `{operators, whitelisted, banned}` name lists |
| PUT | `/api/v1/servers/{id}/players/{username}/access` | Bearer | `{operator?, whitelisted?, banned?}` → updated lists (RCON online, JSON offline) |
| POST | `/api/v1/servers/{id}/command` | Bearer | raw RCON command → `{command, output}` |
| POST | `/api/v1/servers/{id}/players/{username}/kick` | Bearer | `{reason?}` → `kick …` |
| POST | `/api/v1/servers/{id}/players/{username}/ban` | Bearer | `{reason?}` → `ban …` |
| POST | `/api/v1/servers/{id}/players/{username}/op` | Bearer | no body → `op …` |
| GET | `/api/v1/servers/{id}/properties` | Bearer | `{path, hash, entries, redacted}` |
| PUT | `/api/v1/servers/{id}/properties` | Bearer | `{entries, expected_hash?}` → new `{path, hash, entries, redacted}` |
| GET | `/api/v1/servers/{id}/directory[/{path}]` | Bearer | `[{name, size, is_dir}]` |
| POST | `/api/v1/servers/{id}/directory/{path}` | Bearer | create empty directory → 201 |
| GET | `/api/v1/servers/{id}/files/{path}` | Bearer | `{path, content, hash}`; `?download=true` returns raw bytes + attachment headers |
| POST | `/api/v1/servers/{id}/files/{path}` | Bearer | raw `application/octet-stream` upload (browser) |
| PUT | `/api/v1/servers/{id}/files/{path}` | Bearer | text JSON `{content, expected_hash?}` or raw `application/octet-stream` upload |
| POST | `/api/v1/servers/{id}/files/rename` | Bearer | `{old_path, new_path}` → 204 |
| POST | `/api/v1/servers/{id}/files/copy` | Bearer | `{source_path, dest_path}` → 204 |
| DELETE | `/api/v1/servers/{id}/files/{path}` | Bearer | delete regular file → 204 |
| GET | `/api/v1/servers/{id}/addons` | Bearer | installed `.jar`/`.zip` files with directory/type/size |
| POST | `/api/v1/servers/{id}/addons/install` | Bearer | `{url, project_type}` → 201 `InstalledAddon`; `modpack` routes to the daemon modpack engine |
| GET | `/api/v1/ws/servers/{id}` | JWT in `?token=` or `bearer.<jwt>` subprotocol | telemetry socket |

Daemon `/daemon/*` endpoints are HMAC-only:

| Method | Path | Body |
| --- | --- | --- |
| POST | `/daemon/servers/{id}/create` | `CreateServerSpec` → 201 |
| POST | `/daemon/servers/import` | multipart `server_id` + `file` **or** `local_path`; streamed digest HMAC; ≤ 10 GiB |
| POST | `/daemon/servers/{id}/start` | optional `{ram_mb, cpu_limit}`; applies Docker limits, then starts |
| POST | `/daemon/servers/{id}/stop` | 204; graceful 35 s stop |
| POST | `/daemon/servers/{id}/restart` | optional `{ram_mb, cpu_limit}`; stop → apply → start |
| POST | `/daemon/servers/{id}/command` | `{command}` → `{output}` |
| GET | `/daemon/servers/{id}/players` | RCON `list` → `["Steve", …]` |
| GET | `/daemon/servers/{id}/players/history` | `usercache.json` entries |
| GET | `/daemon/servers/{id}/players/access` | `{operators, whitelisted, banned}` |
| PUT | `/daemon/servers/{id}/players/{username}/access` | `{operator?, whitelisted?, banned?}` → updated lists |
| GET | `/daemon/servers/{id}/stats` | NDJSON `StatsSample` |
| GET | `/daemon/servers/{id}/logs` | NDJSON `LogLine` |
| POST | `/daemon/servers/{id}/files/rename` | `{old_path, new_path}` → 204 |
| POST | `/daemon/servers/{id}/files/copy` | `{source_path, dest_path}` → 204 |
| POST | `/daemon/servers/{id}/downloads` | `{url, filename, target_directory}` → `{path, bytes}` (streamed, ≤ 50 MiB) |
| POST | `/daemon/servers/{id}/modpacks/install` | `{url}` → `{name, files, overrides, bytes}` (streamed `.mrpack`, ≤ 200 MiB) |
| GET | `/daemon/servers/{id}/files/{*path}` | `{path, content, hash}` |
| PUT | `/daemon/servers/{id}/files/{*path}` | `{content, expected_hash?}` → `{path, content, hash}` |
| DELETE | `/daemon/servers/{id}/files/{*path}` | 204; regular files only |
| GET | `/daemon/servers/{id}/directory[/{*path}]` | `[{name, size, is_dir}]` |
| POST | `/daemon/servers/{id}/directory/{*path}` | 201; create empty directory |
| GET | `/daemon/servers/{id}/raw-files/{*path}` | raw bytes (≤ 100 MiB) |
| POST | `/daemon/servers/{id}/raw-files/{*path}` | raw bytes body (≤ 100 MiB) |
| PUT | `/daemon/servers/{id}/raw-files/{*path}` | raw bytes body (≤ 100 MiB) |
| DELETE | `/daemon/servers/{id}` | 204; force-remove container, keep `/data` |

## 5. File sandbox & properties parser (M6)

**Path validation (`daemon/src/files.rs`).** Paths are relative to
`<MC_DATA_ROOT>/<server_id>/data`. The daemon rejects empty file paths, NUL bytes, absolute paths,
`Component::RootDir`/`Prefix`, and any `..` component. It canonicalises the server root and every
existing parent/target, then requires `candidate.starts_with(canonical_root)`; this blocks symlink
escapes as well as lexical traversal. Reads must resolve to a regular file and are limited to
1 MiB UTF-8. Writes use a sibling temp file (`.<name>.tmp-<uuidv7>`), `fsync`, then `rename`, with a
cleanup on failure. An optional `expected_hash` is compared before writing and returns `409`.
Directory listings accept the empty path (volume root), require the target to be a directory, and
omit symlinks whose canonical target escapes the volume. Directory creation uses the same
parent/target checks and refuses to overwrite an existing file. Deletion resolves the target under
the same checks and only removes regular files, never directories. Rename resolves the source as an
existing path and the destination as a new path, rejecting a missing parent, an existing destination,
or any escape on either side. Copy applies the same pair of checks, refuses symlinks in the source
tree, copies directories recursively, and caps the total at 100 MiB. Raw read/write endpoints reuse
the sandbox and cap transfer at 100 MiB; text endpoints stay at 1 MiB. Both the API and daemon raise
Axum's default body limit on the upload routes so large binaries are accepted. The daemon's HMAC
guard keeps its 256 KiB control-payload cap everywhere except `PUT /raw-files/*`, which uses the
100 MiB upload budget.

**Browser boundary.** Properties routes use the constant `server.properties`. The File Manager
accepts user-chosen *relative* paths only; absolute host paths never cross the API. Daemon paths are
percent-encoded per segment before signing so the HMAC covers exactly the URI the daemon verifies.

**Server runtime metadata.** Migration `0003_forge_and_game_version.sql` rebuilds `servers` to widen
the `server_type` check to include `forge` and add `game_version TEXT NOT NULL DEFAULT 'LATEST'`.
The API normalises the version (letters, digits, `.`, `-`, `_`, `+`; max 32 chars) and the daemon
maps it to the itzg image's `VERSION` environment variable.

**Parser (`api/src/properties.rs`).** The document is modelled as ordered
`comment | blank | property` entries, so comments, spacing and unknown keys survive a save. It
implements Java semantics: `#`/`!` comments, `=`/`:`/whitespace separators, escaped separators,
`\t \n \r \f \\ \uXXXX`, and backslash-continued logical lines. Serialisation escapes keys/values
symmetrically; non-ASCII is emitted as `\uXXXX` (surrogate pairs included). Validation caps entries
at 2000, keys at 256 chars, rejects empty keys and control characters, and rejects `\n`, `\r`, `=`
inside keys. `rcon.password` is blanked in GET responses and copied from the on-disk document during
PUT, so saving the form cannot erase the RCON credential.

## 6. Modrinth App Store (M8 + M8.1 + M8.5)

**Daemon downloader (`daemon/src/files.rs`).** `POST /daemon/servers/{id}/downloads` accepts
`{url, filename, target_directory}`. The URL must be HTTPS, must not carry credentials, and must not
resolve to a literal loopback/private address or a `.local`/`.internal` host; redirects are disabled
so a public link cannot bounce into the cluster. The directory chain is created component by
component: existing components are canonicalised and checked against the server root, missing ones
are created, and symlinked ancestors must stay inside the volume. The body is streamed with
`reqwest` into `.<name>.download-<uuidv7>`, the running byte count is capped at **50 MiB** (a
declared `Content-Length` over the cap is rejected before writing), then the temp file is flushed,
synced and atomically renamed. Existing regular files may be replaced; a directory or symlink at the
destination is rejected.

**API routing (`api/src/routes/addons.rs`).** `POST /api/v1/servers/{id}/addons/install` only
accepts `https://…modrinth.com` version file URLs, derives the filename from the last path segment,
and requires `.jar` for mods/plugins, `.zip` for datapacks/resource packs and `.mrpack` for
modpacks. The destination is then resolved from the server kind:

| Project type | Destination | Constraint |
| --- | --- | --- |
| mod | `mods/` | Fabric or Forge only |
| plugin | `plugins/` | Paper, Velocity or BungeeCord only |
| datapack | `<level-name>/datapacks/` | `level-name` parsed from `server.properties`, default `world` |
| resourcepack | `plugins/Geyser-Spigot/packs/` or `resourcepacks/` | Geyser folder detected via the daemon directory listing |
| modpack | `mods/` plus pack overrides | Fabric or Forge only; forwarded to the daemon modpack engine |

`level-name` is validated as a single simple directory name (no separators, `..` or controls) before
it is joined into a path. `GET /api/v1/servers/{id}/addons` scans the same applicable directories
(`mods`, `plugins`, `<level>/datapacks`, and the resource-pack folder), keeps `.jar`/`.zip` files,
and returns `{name, path, directory, size, project_type}` sorted by folder then name.

**Daemon modpack engine (`daemon/src/modpacks.rs`).** `POST /daemon/servers/{id}/modpacks/install`
accepts `{url}`. The URL must be HTTPS, public, and end in `.mrpack`. The archive is streamed to
`.<uuid>.mrpack` inside the server volume with a **200 MiB** cap, then `modrinth.index.json` is
parsed in a blocking worker (index cap 4 MiB). Entries whose `env.server` is `unsupported` are
skipped; every remaining entry must have a downloads URL that passes the same HTTPS/public-host
validation as the single-file downloader. Index paths are flattened to unique basenames under
`mods/`, so duplicate filenames are rejected rather than silently overwritten. Files download
through a `tokio::task::JoinSet` with **12** concurrent workers, each writing a sibling temp file
(100 MiB per-file cap) before an atomic rename; symlink and directory destinations are refused.
Afterwards every `overrides/` entry is extracted into a staging directory inside the volume
(100 MiB per file, 512 MiB total, 20 000 entries, symlinks rejected), then applied over the server
root with the standard path sandbox, overwriting regular files. The temp archive and staging
directory are always removed. The API client uses a 900-second per-request timeout for this call.

**M8.5 frontend contract.** Modrinth calls stay browser-side: `/search`, `/project/{id}` and
`/project/{id}/version`. The details modal derives two bounded dropdowns from the project's own
versions for every project type. **Loader / Software** options are the loaders present on versions
that also carry a file of the expected extension, intersected with what the server can run
(Fabric/Forge for mods and modpacks; Paper-compatible `paper|spigot|bukkit|purpur|folia` for plugins
on Paper; exact proxy names; a synthetic `minecraft` loader for datapacks/resource packs).
**Game Version** options are every `game_versions` entry tagged by a file-bearing version for the
chosen loader, deduplicated and sorted numerically descending. The dropdowns default to the
server's loader and exact `game_version` when present, otherwise the first option. Changing either
dropdown re-derives the game-version list and resolves the first `release` matching the selected
loader + game version (falling back to the first match). The selected file URL is what reaches
`/addons/install`. Modpack installs show a persistent **Installing Modpack (This will take a few
minutes)…** banner with a spinner because the daemon call can run for minutes. The installed list is
matched back to cards through a per-server `localStorage` map (`cluster.addons.projects.<serverId>`)
from project ID to installed filename, falling back to normalised slug/title substring matching
against filenames in the current project type's directories. Modpack cards therefore do not flip to
**Delete**: the Installed list contains the individual mods a pack downloaded, not a pack marker.

## 7. Server import & production deployment (M8.8)

**Daemon import engine (`daemon/src/imports.rs`).** `POST /daemon/servers/import` accepts multipart
form data with a required `server_id` (UUID) and exactly one source: a `file` ZIP part or a
`local_path` text field. The route has a **10 GiB** Axum body limit. ZIP bytes are streamed straight
to a temp file under `<MC_DATA_ROOT>/.imports` and removed by a `Drop` guard; local imports read the
directory in place and never modify the source.

The destination is `<MC_DATA_ROOT>/<server_id>/data`, must be empty, and is canonicalised and
checked against the canonical `MC_DATA_ROOT` before use. ZIP entries reject absolute paths, `..`,
NULs, control characters and symlinks; duplicates and non-regular files are rejected. A single
top-level wrapper folder (`survival/...`) is stripped only when it is the only root, has no root
files, and contains a recognisable marker (`server.properties`, `server.jar`, `plugins/`, `mods/`,
`config/`, etc.). Local paths must be absolute, exist, be a directory, and must not be `/`, `/proc`,
`/sys`, `/dev`, `/boot` or `/run`; the recursive copy rejects symlinks, non-regular files and
duplicates. Caps: 100 GiB per file, 250 GiB total, 2 000 000 entries. Any failure clears the
partially imported destination so a retry starts from an empty volume.

**Streaming HMAC.** Multi-gigabyte bodies cannot be buffered, so the API pre-computes
`sha256(multipart_body)`, sends it in `x-cluster-body-sha256`, and signs the canonical string with
that digest. The daemon verifies the signature *before* the handler runs, then hashes the body as it
streams to the multipart parser and rejects a mismatch or a byte count above the cap. The normal
256 KiB control-payload HMAC path is unchanged for every other route.

**API import (`api/src/routes/servers.rs`).** `POST /api/v1/servers/import` accepts the multipart
configuration fields (`name`, `server_type`, `game_version`, `ram_mb`, `cpu_limit`) and exactly one
source. It validates with the same rules as create, auto-allocates the first free game port from
25565, inserts the server as `provisioning`, and creates the stopped container **before** reading a
potentially huge upload so Docker/image failures fail fast. It then streams the ZIP (six-hour
timeout) or forwards `local_path` to the daemon, sets the row to `stopped`, and returns the normal
`ServerView`. On any import failure it removes the container, deletes the row and surfaces the
daemon error. Uploaded ZIP temp files use the OS temp directory and a `Drop` guard.

**Frontend wizard (`features/servers/ImportServerWizard.tsx`).** Three steps: **Source** (Upload ZIP
or Local Path), **Configuration** (name, Paper/Vanilla/Fabric/Forge/Velocity/BungeeCord card, game
version with suggestions, RAM 1–16 GB, CPU 0.5–4 vCPU) and **Import** (review plus the persistent
**Importing and extracting server (this may take a while)…** spinner banner). The Dashboard and
Servers page both expose **Import Server** next to **New Server**; a successful import closes the
wizard and reloads the list. `apiFetch` now leaves the `Content-Type` unset for `FormData`, letting
the browser add the multipart boundary.

**Deployment assets (`deployment/`).**

- `nginx.conf`: port 80, `auth_basic` for the whole vhost, SPA root `/var/www/cluster-panel`,
  `/api/` proxy to `127.0.0.1:3000` with WebSocket `Upgrade`/`Connection`, `client_max_body_size 10g`,
  six-hour read/send timeouts and `proxy_request_buffering off` for imports. A `map` at the top of
  the site file supplies `$connection_upgrade`.
- `cluster-api.service`: dedicated `cluster` user, `/opt/cluster-console/bin/cluster-api`,
  `EnvironmentFile=/etc/cluster-console/api.env`, read-write only `/var/lib/cluster-console`,
  hardened with `ProtectSystem=strict` and restart-on-failure.
- `cluster-daemon.service`: runs as root with `Group=docker` because it mounts the Docker socket and
  owns `/srv/minecraft`; `EnvironmentFile=/etc/cluster-console/daemon.env`, `Requires=docker.service`,
  restart-on-failure. `ProtectHome` is intentionally off so local-path imports can read a server
  directory under `/home`.
- `README.md`: build/install commands, users and directories, both env files, `.htpasswd`, Nginx
  enablement, systemd enablement, first-login migration flow, import limits and upgrade steps.



The detail page now has `Console`, `Players`, `Configuration` and `File Manager` tabs.
`ConfigurationPanel` loads `{entries, hash, redacted}`, keeps the last hash for optimistic
concurrency, and marks the header `unsaved`/`synced`. Known keys render in six sections (Identity,
Gameplay, World, Network, Security & RCON, Performance) using `lib/properties.ts` metadata:
glassmorphic switches for booleans, native selects for enums, neon sliders or boxed number inputs
for numbers, text inputs for strings, and a locked placeholder for `rcon.password`. Keys outside the
schema render in a "Custom properties" card. The form only edits keys already present in the file,
so a save never injects curated defaults that Minecraft would otherwise leave implicit. On success a
restart-required modal explains that the values are on disk but only apply after a container
restart. Reload warns before discarding dirty edits.

M6.5 config fixes: blank optional numeric fields are skipped during validation instead of being
coerced to `0`, `pause-when-empty-seconds` accepts Minecraft's `-1`, the boolean toggle thumb uses
flex centring with symmetrical travel, and the section cards sit in one `flex flex-col gap-6`
wrapper for uniform spacing.

`FileManagerPanel` lists the server's data volume through the directory endpoint. Folders open on
click with a breadcrumb trail; recognised text formats (`.yml`, `.json`, `.properties`, `.txt`, plus
`.conf`, `.toml`, `.mcmeta`, `.md`, `.log`, `.sh`) open in a modal `<textarea>` editor that saves
through the hash-checked file PUT. Every row has **Rename**, a **More** menu with **Copy**, **Move**
and **Delete**, plus **Edit**/`Download` for files. Copy and Move prompt for a destination path
relative to `/data`; rename prompts for a single new name. Existing destinations are never
overwritten. The Name and Size column headers toggle ascending/descending sort (folders stay first).
The header provides **Upload File** (multi-select, raw binary POST), **New File** (creates then opens
the editor), **New Folder**, and **Refresh**. The list reloads after each mutation; the editor
surfaces `409` conflicts and supports Cmd/Ctrl+S.

`ResourcesSection` sits at the top of the Configuration tab and offers wizard-style RAM
(512 MB–64 GB) and CPU (0.5–32 vCPU) sliders with live formatted values and **Save Resources**. It
PATCHes the API, updates the parent server view, and explains that running containers pick up the
new limits on their next restart.

The **Players** tab merges the live RCON roster with `usercache.json`. While the server runs it polls
`list` every 6 seconds; history and access lists are fetched in the same pass, so offline players
remain visible when the container stops. Cards show a green **Online** or gray **Offline** dot/badge,
op/whitelist/ban badges, and work offline: Kick stays online-only, while Ban/Unban and Op/Deop call
the access endpoint. The **Player Inspector** drawer replaces the old fake Health/Hunger/Inventory
panels with real **Operator**, **Whitelist** and **Ban** toggles. Those toggles use RCON on a running
server and edit `ops.json`, `whitelist.json` and `banned-players.json` while it is stopped. Ping,
gamemode and session remain unknown until the companion plugin lands.

The **App Store** tab searches `api.modrinth.com/v2/search` directly from the browser. A project-type
dropdown is strictly bounded by the server kind: Fabric/Forge offer Mods + Modpacks + Datapacks +
Resource Packs and default to Mods; Paper/Velocity/BungeeCord offer Plugins + Datapacks + Resource
Packs and default to Plugins; every other kind defaults to Datapacks. Facets are built dynamically:
`project_type:<kind>` plus `categories:fabric`/`forge` for mods and modpacks,
`categories:paper`/`velocity`/`bungeecord` for plugins. Results render as cards (icon, title,
author, download count, categories). Clicking a card opens a modal that fetches the full project
body (rendered as GitHub-flavoured Markdown through `react-markdown` + `remark-gfm`), its
categories and a link back to Modrinth. The modal's footer has **Loader / Software** and **Game
Version** dropdowns derived from the project's versions, defaulted to the server's loader and
`game_version`, plus the exact resolved release underneath. Modpack installs show a
**Installing Modpack (This will take a few minutes)…** spinner banner. The card **Install** button
quick-installs the latest compatible version with a per-card spinner.

Cards cross-reference `GET /api/v1/servers/{id}/addons`: an installed project shows a red **Delete**
instead of **Install**, and deleting through the existing file endpoint immediately returns the card
to **Install**. A **Discover / Installed** toggle keeps an always-visible Installed sub-tab that
groups every discovered jar/zip by folder (`plugins/`, `mods/`, `<world>/datapacks/`, resource-pack
folder) with file size, type badge and per-file Delete. Notices remind the user to restart the
server so new files load.

The detail header adds an amber **Restart** button beside Start/Stop. It calls the restart endpoint,
which marks `stopping`, asks the daemon to stop (35 s grace), apply the latest limits, and start
again.

The create wizard now includes a **Forge** card and a **Game version** field. The field defaults to
`LATEST`, accepts free text, and offers a datalist of common releases plus `SNAPSHOT`.

## 8. Known limitations / security notes

- The RCON `list` command returns usernames only; ping, gamemode, session time, vitals and inventory
  still require the companion plugin. M6.7 removed the fake vitals/inventory UI rather than imply
  they work.
- Offline access changes require the player's UUID from `usercache.json`. A name that has never
  joined cannot be added to `ops.json`/`whitelist.json`/`banned-players.json` while stopped; the API
  returns `409` with a clear message. Online changes use RCON and do not need the cache.
- No diff preview or property search in the config UI; a separate whitelist/ops/bans list editor is
  not built (the per-player toggles cover the common workflow).
- Proxy-network CRUD, proxy config files and durable jobs remain deferred.
- Changing `server-port`/RCON ports through the editor does not migrate Docker port mappings; the
  container is recreated from the wizard/daemon contract, not from `server.properties`.
- RCON host ports are derived, not stored. A collision is unlikely at v1 scale but possible.
- RCON is reachable by any local process on the host (`127.0.0.1`); host-level access already
  implies Docker access.
- Existing containers created before M5 do not have RCON env; recreate them to use commands.
- The JWT is in the WebSocket query string, so it may appear in proxy access logs (M7 hardening).
- `JWT_SECRET` / `DAEMON_SHARED_SECRET` still fall back to development defaults with warnings.
- Server deletion intentionally keeps `<MC_DATA_ROOT>/<serverId>/data`.
- Text writes are capped at 1 MiB; raw uploads/downloads are capped at 100 MiB.
- Resource changes are persisted immediately but only reach Docker on the next start/restart; the UI
  tells the user to restart a running server. There is no live hot-resize path.
- Folders can be renamed, copied and moved, but folder deletion is still intentionally unavailable.
- Modpack installs are not transactional. A failed pack can leave already-downloaded mods and applied
  overrides on disk; there is no rollback, dependency solver or loader-version pinning beyond
  Modrinth's version metadata.
- Modpack files are flattened into `mods/` by basename; two different pack paths with the same
  filename are rejected instead of nested. Only the first `downloads` URL is used, and
  `env.client`-only entries with no `env.server` are downloaded (harmless on a dedicated server but
  not client-filtered).
- A modpack install is a single long daemon request. Large packs can take minutes, and the browser
  tab must stay open; there is no resumable job or progress bar yet.
- Installed modpacks are not tracked as a unit. The Installed view lists the individual downloaded
  mods, so a pack card does not flip to **Delete** and uninstalling a pack means removing its files.
- The modpack archive cap is 200 MiB and each indexed file is capped at 100 MiB; overrides are
  capped at 512 MiB total. A pack above those limits is rejected.
- Installed add-ons are read from disk, not from a database. Files placed by other means still
  appear in the list, and a deleted file disappears on the next refresh.
- Card install-state matching is heuristic: a project ID → filename map in per-server
  `localStorage` is authoritative, with normalised slug/title substring matching as a fallback.
  Renaming an installed jar outside the UI, clearing browser storage, or two projects with very
  similar names can therefore produce a missed or false match.
- The App Store bundle now exceeds Vite's 500 kB chunk warning because of the Markdown renderer.
  Code-splitting the modal is a polish item, not a correctness issue.
- Modrinth calls are made from the browser, so the user needs network access to `api.modrinth.com`
  even though installs stream through the daemon.
- Add-on installs use the direct Modrinth CDN file URL and reject redirects. If Modrinth changes its
  CDN hostname policy, the URL allow-list in `api/src/routes/addons.rs` and the daemon's HTTPS-only
  policy must stay in sync.
- Resource-pack routing chooses Geyser's `packs/` folder whenever `plugins/Geyser-Spigot` exists,
  even if Geyser is disabled; a non-Geyser server uses `resourcepacks/`.
- The download host check blocks loopback/private IP literals and `.local`/`.internal` names, but it
  is not a full DNS-rebinding defence; the API separately restricts installs to Modrinth hosts.
- `game_version` is stored and mapped to `VERSION`, but changing it requires recreating the
  container; it is not editable after provisioning.
- Older containers need a recreate to pick up the new `VERSION` env var and Forge support.

## 9. Repository state

- Last commit: `99135af feat: M8 advanced app store with strict contextual filtering, markdown details, and reactive install states`.
- M8.5 backend/frontend/docs changes are **uncommitted**.
- New backend files: `daemon/src/files.rs`, `daemon/src/players.rs`, `daemon/src/modpacks.rs`,
  `api/src/properties.rs`, `api/src/routes/properties.rs`, `api/src/routes/files.rs`,
  `api/src/routes/addons.rs`, `migrations/0003_forge_and_game_version.sql`.
- New frontend files: `lib/properties.ts`,
  `features/server-detail/ConfigurationPanel.tsx`,
  `features/server-detail/FileManagerPanel.tsx`,
  `features/server-detail/AppStorePanel.tsx`.
- `.env.example` has a local `JWT_SECRET` edit and is intentionally excluded from commits.
- `frontend/dist/` is generated by verification and ignored.
- New frontend npm dependencies for M8.1: `react-markdown` and `remark-gfm` (Markdown rendering in
  the project-details modal); both are listed in `package.json`/`package-lock.json`.
- M8.5 adds the workspace `zip` dependency (`zip v2.6.1` with the `deflate` feature) for `.mrpack`
  parsing; the lockfile gained `arbitrary`, `derive_arbitrary`, `zip` and `zopfli`.
- The daemon now depends on the already-pinned workspace `reqwest` (rustls + stream); no other new
  crates were added beyond that dependency edge.

## 10. How to resume

```bash
# terminal 1 — daemon (needs Docker for lifecycle; file endpoints work without a container)
cd backend
DAEMON_SHARED_SECRET=devsecret DAEMON_ADDR=127.0.0.1:9090 \
  MC_DATA_ROOT=./data/servers cargo run -p cluster-daemon

# terminal 2 — API
ADMIN_PASSWORD='change-me' DATABASE_PATH=./data/control.db JWT_SECRET=dev-jwt \
  DAEMON_SHARED_SECRET=devsecret DAEMON_URL=http://127.0.0.1:9090 \
  cargo run -p cluster-api

# terminal 3 — SPA (proxies /api incl. WS to :8080)
cd frontend
npm run dev
```

Create or seed a server, ensure `<MC_DATA_ROOT>/<id>/data/server.properties` exists, then open the
detail page → **Configuration** or **File Manager**. Run `npm run build` before handing off UI
changes.

Next: **run the M8.5 review pass** against the patched build, including Modpack filter visibility
(Fabric/Forge only), a real `.mrpack` install with the long-running banner, the derived Loader /
Game Version dropdowns for every project type, and the exact-release resolution. Stop after
verification and wait for user approval. Do not start M9 (backups), multi-node or the companion
plugin.
