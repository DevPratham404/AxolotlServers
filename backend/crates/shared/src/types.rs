//! Server types and the payload the control plane sends when provisioning.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A managed server kind. Proxies are first-class members of this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
    Vanilla,
    Paper,
    Fabric,
    Forge,
    Velocity,
    Bungeecord,
    Bedrock,
    NeoForge,
    Quilt,
    Spigot,
    Purpur,
    Pufferfish,
    Folia,
    Sponge,
    Mohist,
    Arclight,
    CatServer,
    Magma,
    Waterfall,
}

impl ServerType {
    pub const ALL: [Self; 19] = [
        Self::Vanilla,
        Self::Paper,
        Self::Fabric,
        Self::Forge,
        Self::Velocity,
        Self::Bungeecord,
        Self::Bedrock,
        Self::NeoForge,
        Self::Quilt,
        Self::Spigot,
        Self::Purpur,
        Self::Pufferfish,
        Self::Folia,
        Self::Sponge,
        Self::Mohist,
        Self::Arclight,
        Self::CatServer,
        Self::Magma,
        Self::Waterfall,
    ];

    /// Value stored in SQLite and sent over the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Paper => "paper",
            Self::Fabric => "fabric",
            Self::Forge => "forge",
            Self::Velocity => "velocity",
            Self::Bungeecord => "bungeecord",
            Self::Bedrock => "bedrock",
            Self::NeoForge => "neoforge",
            Self::Quilt => "quilt",
            Self::Spigot => "spigot",
            Self::Purpur => "purpur",
            Self::Pufferfish => "pufferfish",
            Self::Folia => "folia",
            Self::Sponge => "sponge",
            Self::Mohist => "mohist",
            Self::Arclight => "arclight",
            Self::CatServer => "catserver",
            Self::Magma => "magma",
            Self::Waterfall => "waterfall",
        }
    }

    /// `TYPE` value understood by the itzg/minecraft-server image.
    pub const fn itzg_type(self) -> &'static str {
        match self {
            Self::Vanilla => "VANILLA",
            Self::Paper => "PAPER",
            Self::Fabric => "FABRIC",
            Self::Forge => "FORGE",
            Self::Velocity => "VELOCITY",
            Self::Bungeecord => "BUNGEECORD",
            Self::Bedrock => "BEDROCK",
            Self::NeoForge => "NEOFORGE",
            Self::Quilt => "QUILT",
            Self::Spigot => "SPIGOT",
            Self::Purpur => "PURPUR",
            Self::Pufferfish => "PUFFERFISH",
            Self::Folia => "FOLIA",
            Self::Sponge => "SPONGEVANILLA",
            Self::Mohist => "MOHIST",
            Self::Arclight => "ARCLIGHT",
            Self::CatServer => "CATSERVER",
            Self::Magma => "MAGMA",
            Self::Waterfall => "WATERFALL",
        }
    }

    /// True for Velocity/BungeeCord proxy containers.
    pub const fn is_proxy(self) -> bool {
        matches!(self, Self::Velocity | Self::Bungeecord)
    }
}

impl fmt::Display for ServerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ServerType {
    type Err = ParseServerTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "vanilla" => Ok(Self::Vanilla),
            "paper" => Ok(Self::Paper),
            "fabric" => Ok(Self::Fabric),
            "forge" => Ok(Self::Forge),
            "velocity" => Ok(Self::Velocity),
            "bungeecord" => Ok(Self::Bungeecord),
            "bedrock" => Ok(Self::Bedrock),
            "neoforge" => Ok(Self::NeoForge),
            "quilt" => Ok(Self::Quilt),
            "spigot" => Ok(Self::Spigot),
            "purpur" => Ok(Self::Purpur),
            "pufferfish" => Ok(Self::Pufferfish),
            "folia" => Ok(Self::Folia),
            "sponge" => Ok(Self::Sponge),
            "mohist" => Ok(Self::Mohist),
            "arclight" => Ok(Self::Arclight),
            "catserver" => Ok(Self::CatServer),
            "magma" => Ok(Self::Magma),
            "waterfall" => Ok(Self::Waterfall),
            other => Err(ParseServerTypeError(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseServerTypeError(pub String);

impl fmt::Display for ParseServerTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown server type: {}", self.0)
    }
}

impl std::error::Error for ParseServerTypeError {}

/// Everything the daemon needs to create a container, as decided by the control
/// plane and persisted in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateServerSpec {
    /// Server id (UUIDv7) — also the container's stable identity.
    pub id: String,
    pub name: String,
    pub server_type: ServerType,
    /// itzg `VERSION` value, e.g. `LATEST`, `1.21.1`, or `SNAPSHOT`.
    #[serde(default = "default_game_version")]
    pub game_version: String,
    /// Host port. Ignored (not published) when `is_behind_proxy` is true.
    pub game_port: u16,
    pub ram_mb: i64,
    pub cpu_limit: f64,
    /// When true the server is reachable only on the Docker network (no host port).
    pub is_behind_proxy: bool,
    #[serde(default = "default_world_name")]
    pub world_name: String,
    #[serde(default)]
    pub world_seed: String,
    #[serde(default = "default_world_type")]
    pub world_type: String,
    #[serde(default = "default_difficulty")]
    pub difficulty: String,
    #[serde(default = "default_gamemode")]
    pub gamemode: String,
}

fn default_game_version() -> String {
    "LATEST".to_string()
}
fn default_world_name() -> String {
    "world".to_string()
}
fn default_world_type() -> String {
    "default".to_string()
}
fn default_difficulty() -> String {
    "normal".to_string()
}
fn default_gamemode() -> String {
    "survival".to_string()
}

/// Validate the itzg `VERSION` selector. Accepts release ids (`1.21.1`),
/// `LATEST`, `SNAPSHOT`, and Forge build labels without whitespace.
pub fn normalize_game_version(raw: &str) -> Result<String, String> {
    const MAX_CHARS: usize = 32;
    let value = raw.trim();
    if value.is_empty() {
        return Err("game_version must not be empty".into());
    }
    if value.chars().count() > MAX_CHARS {
        return Err(format!(
            "game_version must be at most {MAX_CHARS} characters"
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        return Err("game_version may only contain letters, digits, '.', '-', '_' and '+'".into());
    }
    if value.eq_ignore_ascii_case("latest") {
        return Ok("LATEST".to_string());
    }
    Ok(value.to_string())
}

/// Runtime resource limits applied to an existing container.
///
/// The control plane owns SQLite, so it sends the current values with every
/// start/restart request; the daemon applies them to Docker's HostConfig.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ServerResources {
    pub ram_mb: i64,
    pub cpu_limit: f64,
}

/// Payload for `POST /daemon/servers/{id}/command`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RconCommandRequest {
    /// Raw Minecraft command, without a leading slash.
    pub command: String,
}

/// Result of a raw command executed over RCON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RconCommandResponse {
    /// Text returned by the server (may be empty).
    pub output: String,
}

/// Result of reading a text file inside a server's `/data` volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    /// Path relative to the server's data root, e.g. `server.properties`.
    pub path: String,
    pub content: String,
    /// `hex(sha256(content))`; used for optimistic concurrency.
    pub hash: String,
}

/// Payload for writing a text file inside a server's `/data` volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteRequest {
    pub content: String,
    /// When present the daemon refuses the write if the on-disk hash differs.
    #[serde(default)]
    pub expected_hash: Option<String>,
}

/// One child of a directory inside a server's `/data` volume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub name: String,
    /// Size in bytes; `0` for directories.
    pub size: u64,
    pub is_dir: bool,
}

/// One entry from `usercache.json` — a player who has joined at least once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerHistoryEntry {
    pub name: String,
    pub uuid: String,
    /// Minecraft's cache expiry timestamp (ISO string in `usercache.json`).
    #[serde(default, alias = "expiresOn")]
    pub expires_on: Option<String>,
}

/// The three name-based access lists shared by the daemon and control plane.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerAccessLists {
    pub operators: Vec<String>,
    pub whitelisted: Vec<String>,
    pub banned: Vec<String>,
}

/// Partial update for a player's access state. Omitted fields stay unchanged.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PlayerAccessUpdate {
    #[serde(default)]
    pub operator: Option<bool>,
    #[serde(default)]
    pub whitelisted: Option<bool>,
    #[serde(default)]
    pub banned: Option<bool>,
}

/// Payload for `POST /daemon/servers/{id}/files/rename`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRenameRequest {
    pub old_path: String,
    pub new_path: String,
}

/// Payload for `POST /daemon/servers/{id}/files/copy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCopyRequest {
    pub source_path: String,
    pub dest_path: String,
}

/// Kind of add-on installed through the Modrinth browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddonProjectType {
    Mod,
    Plugin,
    Datapack,
    Resourcepack,
    Modpack,
}

impl AddonProjectType {
    pub const ALL: [Self; 5] = [
        Self::Mod,
        Self::Plugin,
        Self::Datapack,
        Self::Resourcepack,
        Self::Modpack,
    ];

    /// Stable wire/DB spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Plugin => "plugin",
            Self::Datapack => "datapack",
            Self::Resourcepack => "resourcepack",
            Self::Modpack => "modpack",
        }
    }

    /// File extension the project kind is distributed as.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mod | Self::Plugin => "jar",
            Self::Datapack | Self::Resourcepack => "zip",
            Self::Modpack => "mrpack",
        }
    }
}

/// Payload for `POST /api/v1/servers/{id}/addons/install`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonInstallRequest {
    /// Direct Modrinth version file URL.
    pub url: String,
    pub project_type: AddonProjectType,
}

/// Payload for `POST /daemon/servers/{id}/downloads`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonDownloadRequest {
    pub url: String,
    /// Single file name; must not contain path separators.
    pub filename: String,
    /// Directory relative to the server data root, e.g. `mods`.
    pub target_directory: String,
}

/// Result of a streamed add-on download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddonDownloadResult {
    /// Volume-relative path of the written file.
    pub path: String,
    pub bytes: u64,
}

/// Payload for `POST /daemon/servers/{id}/modpacks/install`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModpackInstallRequest {
    /// Direct Modrinth version file URL pointing at a `.mrpack`.
    pub url: String,
}

/// Summary of a completed `.mrpack` install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModpackInstallResult {
    /// Filename derived from the requested URL.
    pub name: String,
    /// Mod files downloaded from the pack index.
    pub files: usize,
    /// Overrides applied from `overrides/`.
    pub overrides: usize,
    /// Total bytes downloaded for mod files.
    pub bytes: u64,
}

/// Summary of a completed server-data import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportServerResult {
    /// Server id whose data directory was populated.
    pub id: String,
    /// Regular files copied or extracted.
    pub files: usize,
    /// Total bytes written.
    pub bytes: u64,
}

/// One installed `.jar`/`.zip` inside an add-on directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledAddon {
    pub name: String,
    /// Volume-relative path, e.g. `plugins/Example.jar`.
    pub path: String,
    /// Parent directory relative to the data root.
    pub directory: String,
    pub size: u64,
    pub project_type: AddonProjectType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_server_types() {
        for server_type in ServerType::ALL {
            assert_eq!(
                server_type.to_string().parse::<ServerType>(),
                Ok(server_type)
            );
        }
        assert_eq!("spigot".parse::<ServerType>(), Ok(ServerType::Spigot));
        assert_eq!("forge".parse::<ServerType>(), Ok(ServerType::Forge));
    }

    #[test]
    fn serializes_to_db_values() {
        assert_eq!(
            serde_json::to_string(&ServerType::Bungeecord).expect("serialize server type"),
            "\"bungeecord\""
        );
        assert_eq!(ServerType::Velocity.itzg_type(), "VELOCITY");
        assert_eq!(ServerType::Forge.itzg_type(), "FORGE");
        assert!(ServerType::Velocity.is_proxy());
        assert!(!ServerType::Paper.is_proxy());
        assert!(!ServerType::Forge.is_proxy());
    }

    #[test]
    fn normalises_game_versions() {
        assert_eq!(normalize_game_version(" latest "), Ok("LATEST".to_string()));
        assert_eq!(normalize_game_version("1.21.1"), Ok("1.21.1".to_string()));
        assert!(normalize_game_version("").is_err());
        assert!(normalize_game_version("1.21 1").is_err());
    }
}
