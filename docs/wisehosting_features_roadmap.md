# WiseHosting Game Panel Feature Breakdown & AxolotlServers Roadmap

**Reference video:** [WiseHosting Game Panel Explained in 5 Minutes](https://youtu.be/Xx5UACDTTiY?si=6bKlTJrMksyO0uHK)  
**Target project:** AxolotlServers (`Cluster Console` — Rust API + Daemon + React/Vite "Aurora Glass" frontend)  
**Last reviewed:** 2026-11-16 · **Current phase:** M8.9.1 + backup-center completion (manual E2E review pending)

> This document started as a gap analysis against the WiseHosting video. It is now a
> parity tracker: every status below reflects what is actually shipped in the repository,
> not what the video advertises. Automated checks pass (`cargo fmt/check/clippy/test`,
> `npm run build/lint`, `npm test` in `mcp/`); the remaining gate is a hands-on pass with
> real Minecraft containers.

---

## Status legend

* ✅ **Built** — implemented and verified by automated checks; manual E2E confirmation may
  still be pending where noted.
* 🚧 **In progress** — a working subset exists, but the full video workflow is not finished.
* ⬜ **Left / planned** — not implemented yet.

---

## 1. Core infrastructure & security (our foundation)

WiseHosting runs on a customized Pterodactyl-style stack. AxolotlServers uses a split
control-plane/daemon architecture with stricter defaults:

* ✅ **Zero-shell server lifecycle:** create, start, stop, restart and delete 19 runtimes
  from the browser — Vanilla, Paper, Spigot, Purpur, Pufferfish, Folia, Fabric, Forge,
  NeoForge, Quilt, Sponge, Mohist, Arclight, CatServer, Magma, Velocity, BungeeCord,
  Waterfall and Bedrock.
* ✅ **Hard Docker resource enforcement:** container-level RAM and CPU limits
  (`Memory == MemorySwap`, `NanoCpus`, `PidsLimit`, `no-new-privileges`), with live
  resizing from the Configuration tab.
* ✅ **HMAC-signed control plane:** the web-facing `cluster-api` never touches the Docker
  socket. Every daemon instruction is signed; streamed imports use digest-based HMAC.
* ✅ **Authentication & bootstrap security:** JWT sessions, SQLite (`control.db`), Nginx
  production profile, forced first-login password rotation and argon2id hashes.
* ✅ **Runtime version discovery:** `/api/v1/runtimes/{type}/versions` resolves the builds
  each runtime actually publishes (Mojang, Arclight/CatServer GitHub, Mohist/Magma APIs),
  including pinned hybrid versions that reject `LATEST`.

---

## 2. Tab-by-tab feature breakdown (from the video)

### Tab 1: Console & live monitoring

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `0:09` | **Live server console** | Real-time terminal output, command input, power controls and live CPU/RAM/disk graphs. | ✅ **Built** | WebSocket multiplexes Docker stats + logs; graphical console parses level/source, filters and pause-on-scroll; Start/Stop/Restart plus an RCON command bar. A separate hard "Kill" action is not exposed. |
| `0:34` | **Live player manager** | Online/offline roster with 1-click OP, De-OP, Kick, Ban, Whitelist and Teleport. | ✅ **Built** (except teleport) | Live RCON `list` polling every 6 s merged with `usercache.json`; op/whitelist/ban work online (RCON) and offline (JSON files); kick/ban actions on cards. Teleport is not surfaced. Vitals/inventory remain companion-plugin work. |

---

### Tab 2: Files, instances & data management

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `0:58` | **Web file manager** | Directory explorer, uploads, drag-and-drop, archive/unarchive, folder creation and inline syntax-highlighted editor. | ✅ **Built** (core) | Browse, breadcrumbs, sort by name/size, create file/folder, upload/download binaries, inline text editor, rename, copy/move and delete — all under a strict traversal/symlink sandbox. Not yet: drag-and-drop, archive/unarchive and syntax highlighting. |
| `1:17` | **Server instances** | Multiple setups under one slot with hot-swap between profiles without losing data. | ⬜ **Left** | Multiple independent servers are supported, but per-server profile snapshots and hot-swapping are not designed yet. Backups are the current data-swap mechanism. |
| `1:32` | **SFTP access** | Dedicated SFTP credentials and a 1-click launch button. | ⬜ **Left** | Planned as a containerised SFTP sidecar or daemon-managed chroot users. |
| `1:50` | **Databases (MySQL)** | 1-click MariaDB databases and phpMyAdmin credentials for plugins. | ⬜ **Left** | Requires managed database containers plus per-server user provisioning. |
| `1:59` | **Automated & manual backups** | Full snapshots, 1-click restore, locked backups and local downloads. | ✅ **Built** (core) | Manual + scheduled ZIP snapshots with RCON `save-off`/`save-all flush`/`save-on`, SHA-256 manifests, verified restore, archive download, per-server keep-last-N retention and delete. Not yet: archive upload, "locked" backups and off-host sync. |

---

### Tab 3: Server properties (visual configurator)

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `2:12` | **Visual `server.properties` editor** | Categorised toggles, dropdowns and sliders for gamemode, difficulty, view/simulation distance, whitelist, online-mode and MOTD. | ✅ **Built** | Java-properties parser preserves comments, blanks, order and unknown keys; schema-driven sections with toggles/enums/sliders; optimistic concurrency via SHA-256; `rcon.password` redacted in responses and restored on save. |

---

### Tab 4: Worlds management

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `2:29` | **World manager** | Upload worlds, generate fresh seeds, switch active worlds, delete dimensions or download world folders. | ⬜ **Left** | The create wizard accepts world name, seed, type, difficulty and gamemode, and a data ZIP can be imported; a dedicated world browser (scan `level.dat`, switch/upload/download dimensions) is not built. The file manager can browse world folders today. |
| `2:29` | **Pre-made world templates** | 1-click curated maps and Shulkercraft downloads. | ⬜ **Left** | Optional curated template index; no code yet. |

---

### Tab 5: Addons & one-click installers

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `2:58` | **Plugin installer** | Searchable Modrinth/Hangar/SpigotMC catalog with 1-click install into `/plugins`. | ✅ **Built** | Modrinth App Store with runtime-aware filtering (Spigot/Purpur/Pufferfish/Folia/Sponge/proxies/hybrids), project details, loader + game-version dropdowns and installed-file management. Hangar/SpigotMC are not wired. |
| `2:58` | **Mod installer** | Modrinth + CurseForge with dependency resolution into `/mods`. | ✅ **Built** | Modrinth mods on Fabric/Forge/NeoForge/Quilt/hybrids, exact-version install, grouped Installed view and Delete. CurseForge and automatic dependency resolution are not included. |
| `2:58` | **Datapack installer** | VanillaTweaks/Modrinth browser into the active world's `/datapacks`. | ✅ **Built** | Modrinth datapacks land under the configured `level-name/datapacks`. VanillaTweaks is not integrated. |
| `2:58` | **One-click presets** | Simple Voice Chat (auto UDP port) and GeyserMC + Floodgate crossplay bundles. | ⬜ **Left** | High-value QoL automation still to build. Resource packs already route to `plugins/Geyser-Spigot/packs/` when Geyser exists. |

---

### Tab 6: Version & modpack changer

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `3:36` | **Server version switcher** | Switch Vanilla/Paper/Purpur/Fabric/Forge/NeoForge/Velocity/BungeeCord across versions in place. | 🚧 **In progress** | All 19 runtimes are selectable at create/import time, with runtime-specific version discovery. In-place change of runtime/version for an existing server (with data migration) is not implemented. |
| `3:36` | **1-click modpack installer** | 15,000+ Modrinth/CurseForge/FTB modpacks with server-pack extraction. | ✅ **Built** (Modrinth) | Streamed `.mrpack` install: index parsing, `env.server = "unsupported"` filtering, 12-way concurrent downloads, `overrides/` application and cleanup. CurseForge and FTB are not wired. |

---

### Tab 7: Settings, automation & access control

| Timestamp | Feature | WiseHosting implementation | AxolotlServers status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `3:59` | **General server settings** | Rename, upload/resize `server-icon.png`, timezone and reinstall core. | ⬜ **Left** | Server name is set at creation; no icon upload/resize, timezone control or reinstall action. |
| `4:12` | **Automated schedules** | Cron tasks for restarts, backups, warnings and console commands. | ✅ **Built** | Recurring backup, start, stop, restart and command tasks at 1–525,600 minute intervals; a 20 s worker claims before running and records last status/error; UI can create, pause, run now and delete. |
| `4:30` | **Network & subdomains** | Allocated TCP/UDP ports and free subdomains (e.g. `play.example.gg`). | ⬜ **Left** | Game ports are auto-allocated and proxies publish the right defaults; custom port allocation, extra port mappings and Cloudflare DNS/SRV automation are not built. |
| `4:42` | **Users & sub-users (RBAC)** | Invite co-owners with granular permissions. | ⬜ **Left** | Single-role authenticated users today. RBAC is part of the M7 hardening milestone. |
| `4:53` | **Startup & JVM flags** | Custom startup command, Java version selector and Aikar/GraalVM/Shenandoah toggles. | 🚧 **In progress** | RAM/CPU scaling and game version are exposed; Java image selection, custom startup command and optimisation-flag toggles are not. |
| `5:04` | **Audit activity log** | Immutable log of commands, edits, power actions and settings changes. | ⬜ **Left** | Planned alongside RBAC (`audit_logs` table + UI). |

---

### Bonus: built-in Minecraft tools suite (`5:10`)

| Tool | Purpose | AxolotlServers status |
| :--- | :--- | :--- |
| **MOTD editor** | Live server-list preview with RGB/hex gradients. | ⬜ **Left** (pure-React component) |
| **Server icon converter** | Crop/resize to a valid `64x64` `server-icon.png`. | ⬜ **Left** (pure-React Canvas component) |
| **RAM & resource calculator** | Recommend RAM/CPU from player count and modpack weight. | ⬜ **Left** (pure-React tool) |
| **Coordinate calculator** | Overworld ↔ Nether coordinates and region-file names. | ⬜ **Left** (pure-React tool) |
| **Skin & UUID downloader** | Mojang skin/UUID lookup. | ⬜ **Left** |
| **Circle / sphere / tick / enchantment calculators** | Building and game-tick utilities. | ⬜ **Left** |
| **NBT editor** | Browser-based `level.dat` / `.nbt` inspector. | ⬜ **Left** |

---

## 3. Delivered beyond the video

These are implemented and are not (or only partly) shown in the WiseHosting tour:

* **19 runtimes**, including hybrids (Mohist, Arclight, CatServer, Magma) with pinned
  installer versions and a runtime-aware App Store.
* **Server import engine** — streamed ZIP (≤ 10 GiB) or daemon-host path, with
  traversal/symlink rejection, wrapper-folder stripping, rollback and a three-step wizard.
* **Modrinth App Store** — mods, plugins, datapacks, resource packs and modpacks, with
  full Markdown project details, loader/game-version dropdowns and reactive install state.
* **ZIP backup center** — retention policy, integrity verification, authenticated archive
  download, verified restore and scheduled snapshots.
* **MCP server (Markdown Context Protocol)** — every repository markdown file is exposed as
  a `cluster://docs/<path>` resource; typed tools wrap servers, backups (including verify
  and retention), schedules and runtime capabilities. See [`../mcp/README.md`](../mcp/README.md).
* **Production deployment profile** — Nginx reverse proxy with WebSocket upgrades, systemd
  units and an operator guide under `deployment/`.
* **Real-time telemetry** — multiplexed stats/log WebSocket, per-server CPU/memory/network/PID
  samples, long-lived streams without a 30 s timeout, and retryable early-RCON errors.

---

## 4. Summary progress matrix

Counted from the 33 video features tracked in sections 2–3.

| Category | Total | ✅ Built | 🚧 In progress | ⬜ Left |
| :--- | :---: | :---: | :---: | :---: |
| **Core runtime, security & auth** | 4 | 4 | 0 | 0 |
| **Console & player management** | 2 | 2 | 0 | 0 |
| **Files, instances, SFTP, DB & backups** | 5 | 2 | 0 | 3 |
| **Visual properties & worlds** | 3 | 1 | 0 | 2 |
| **Addons, mods, plugins & version switcher** | 6 | 4 | 1 | 1 |
| **Schedules, network, sub-users & audit** | 6 | 1 | 1 | 4 |
| **Built-in Minecraft tools suite** | 7 | 0 | 0 | 7 |
| **TOTAL** | **33** | **14 (42%)** | **2 (6%)** | **17 (52%)** |

---

## 5. Recommended next build order

0. **Gate: manual end-to-end review (current blocker).** Run the M6.5–M8.9.1 checklists in
   [`todo.md`](todo.md) against real Minecraft containers: create each runtime category,
   stream console/stats for several minutes, install a mod/plugin/datapack/modpack, take +
   verify + download + restore a backup, run a scheduled command, and exercise the MCP tools.
   Fix release blockers before any new milestone.
1. **Complete in-progress settings (small, high leverage).**
   * In-place runtime/version switch with a safety backup and data migration.
   * Java image selector, custom startup command and Aikar/optimisation flag toggles.
   * General settings: rename, `server-icon.png` upload/resize, timezone, reinstall core.
2. **World manager.** Scan `level.dat` folders, upload/download/switch worlds, delete
   dimensions, then add a curated world-template index.
3. **Automation & access control (M7/M9).** RBAC on REST + WS, audit log, retention uploads
   and off-host backup sync, rate limiting, CSRF stance, graceful shutdown and metrics.
4. **Ecosystem expansion.** SFTP sidecar, managed MariaDB provisioning, custom port
   mappings + Cloudflare subdomains, one-click Geyser/Voice Chat presets, CurseForge/FTB.
5. **Tools suite & polish.** MOTD editor, icon converter, calculators, NBT inspector;
   i18n, accessibility, reduced-motion and mobile layout.
6. **M10 multi-node.** `nodes` table + `servers.node_id`, enrollment UI, daemon TLS/mTLS,
   node metrics and capacity dashboard.

---

## 6. Markdown Context Protocol (MCP)

The repository ships an MCP server in [`mcp/`](../mcp/README.md) that turns documentation
into first-class context:

* **Resources:** every tracked `*.md` (including this file) is exposed as
  `cluster://docs/<path>` and re-read on every request, so edits are visible without a
  restart. `list_documents` / `read_document` support clients without resource support.
* **Tools:** server lifecycle, backups (`create`, `list`, `restore`, `delete`, `verify`,
  retention), schedules and runtime capability descriptions, annotated with read-only and
  destructive hints.
* **Prompts:** `server_triage` and `disaster_recovery` guide safe, read-first investigation.

When this roadmap changes, no extra MCP registration is required — the doc is discovered
automatically. Update the tool table in `mcp/README.md` when tools change.
