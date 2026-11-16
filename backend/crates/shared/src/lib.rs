//! Types and protocol helpers shared by the control plane and the node daemon.

pub mod hmac_auth;
pub mod telemetry;
pub mod types;

pub use hmac_auth::{
    HMAC_BODY_SHA256_HEADER, HMAC_SIGNATURE_HEADER, HMAC_TIMESTAMP_HEADER, MAX_CLOCK_SKEW_SECS,
    sign, sign_digest, verify, verify_digest,
};
pub use telemetry::{LogLine, LogStream, StatsSample, TelemetryMessage};
pub use types::{
    AddonDownloadRequest, AddonDownloadResult, AddonInstallRequest, AddonProjectType,
    BackupCreateRequest, BackupInfo, BackupVerification, CreateServerSpec, DirectoryEntry,
    FileContent, FileCopyRequest, FileRenameRequest, FileWriteRequest, ImportServerResult,
    InstalledAddon, ModpackInstallRequest, ModpackInstallResult, PlayerAccessLists,
    PlayerAccessUpdate, PlayerHistoryEntry, RconCommandRequest, RconCommandResponse,
    ServerResources, ServerType, normalize_game_version,
};

use serde::{Deserialize, Serialize};

/// Version of the control-plane HTTP/WS contract. Bumped on breaking changes.
pub const API_VERSION: &str = "v1";

/// Version of the daemon <-> control-plane control channel.
pub const CONTROL_PROTOCOL_VERSION: u32 = 1;

/// Type-safe entity identifier (UUIDv7, time-sortable).
pub type EntityId = uuid::Uuid;

/// Uniform error body returned by every non-2xx API response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorBody {
    /// Stable machine-readable code, e.g. `server_not_found`.
    pub code: String,
    /// Human-readable message safe to display in the UI.
    pub message: String,
}

impl ApiErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
