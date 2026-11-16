//! Server backups: zip the whole `/data` volume next to it, list, verify,
//! restore and delete. Backups intentionally live outside `/data` so taking a
//! snapshot never includes previous archives.
//!
//! Archives are ZIP files written with deflate. Every path is bounded and
//! validated, symlinks are skipped while backing up and rejected on restore,
//! and a SHA-256 digest stored in a sidecar lets restore detect corruption.

use std::io::Read;
use std::path::{Path, PathBuf};

use cluster_shared::{BackupInfo, BackupVerification};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::error::DaemonError;

const MAX_BACKUP_BYTES: u64 = 250 * 1024 * 1024 * 1024;
const MAX_BACKUP_FILE_BYTES: u64 = 100 * 1024 * 1024 * 1024;
const MAX_BACKUP_ENTRIES: usize = 2_000_000;

/// Create a ZIP snapshot of `<data_root>/<server_id>/data`.
pub async fn create_backup(
    data_root: &str,
    server_id: &str,
    name: Option<String>,
    server_was_running: bool,
) -> Result<BackupInfo, DaemonError> {
    let server_root = server_root(data_root, server_id)?;
    let data = server_root.join("data");
    if !data.is_dir() {
        return Err(DaemonError::NotFound(
            "server data directory does not exist".into(),
        ));
    }
    let backups = server_root.join("backups");
    tokio::fs::create_dir_all(&backups)
        .await
        .map_err(map_io("create backup directory"))?;

    let id = uuid::Uuid::now_v7().to_string();
    let archive = backups.join(format!("{id}.zip"));
    let label = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(default_label);
    let label: String = label.chars().take(120).collect();
    let created_at_ms = now_ms();

    let data_clone = data.clone();
    let archive_clone = archive.clone();
    tokio::task::spawn_blocking(move || write_archive(&data_clone, &archive_clone))
        .await
        .map_err(|error| DaemonError::Internal(format!("backup task failed: {error}")))??;

    let (size_bytes, sha256) = hash_file(&archive).await?;
    let info = BackupInfo {
        id,
        name: label,
        size_bytes,
        sha256,
        created_at_ms,
        server_was_running,
    };
    write_sidecar(&backups, &info).await?;

    tracing::info!(
        server_id = %server_id,
        backup_id = %info.id,
        bytes = info.size_bytes,
        "created server backup"
    );
    Ok(info)
}

/// List every valid backup sidecar. Corrupt or half-written entries are skipped.
pub async fn list_backups(
    data_root: &str,
    server_id: &str,
) -> Result<Vec<BackupInfo>, DaemonError> {
    let server_root = server_root(data_root, server_id)?;
    let backups = server_root.join("backups");
    let mut entries = match tokio::fs::read_dir(&backups).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(map_io("list backup directory")(error)),
    };

    let mut infos = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(map_io("list backup directory"))?
    {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(&path).await else {
            continue;
        };
        let Ok(info) = serde_json::from_slice::<BackupInfo>(&bytes) else {
            continue;
        };
        if backups.join(format!("{}.zip", info.id)).is_file() {
            infos.push(info);
        }
    }
    infos.sort_by_key(|backup| std::cmp::Reverse(backup.created_at_ms));
    Ok(infos)
}

/// Replace the live `/data` directory with the content of a verified archive.
pub async fn restore_backup(
    data_root: &str,
    server_id: &str,
    backup_id: &str,
) -> Result<BackupInfo, DaemonError> {
    validate_id(backup_id)?;
    let server_root = server_root(data_root, server_id)?;
    let backups = server_root.join("backups");
    let archive = backups.join(format!("{backup_id}.zip"));
    if !archive.is_file() {
        return Err(DaemonError::NotFound("backup not found".into()));
    }
    let info = read_sidecar(&backups, backup_id).await?;
    let (_, actual) = hash_file(&archive).await?;
    if !actual.eq_ignore_ascii_case(&info.sha256) {
        return Err(DaemonError::BadRequest(
            "backup archive failed its SHA-256 integrity check".into(),
        ));
    }

    let staging = server_root.join(format!(".restore-{}", uuid::Uuid::now_v7()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(map_io("create restore staging directory"))?;

    let archive_clone = archive.clone();
    let staging_clone = staging.clone();
    let extract =
        tokio::task::spawn_blocking(move || extract_archive(&archive_clone, &staging_clone))
            .await
            .map_err(|error| DaemonError::Internal(format!("restore task failed: {error}")))?;
    if let Err(error) = extract {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(error);
    }

    let data = server_root.join("data");
    let previous = server_root.join(format!(".data-before-restore-{}", uuid::Uuid::now_v7()));
    if data.exists() {
        tokio::fs::rename(&data, &previous)
            .await
            .map_err(map_io("move current data aside"))?;
    }
    if let Err(error) = tokio::fs::rename(&staging, &data).await {
        if previous.exists() {
            let _ = tokio::fs::rename(&previous, &data).await;
        }
        return Err(map_io("activate restored data")(error));
    }
    if previous.exists() {
        let _ = tokio::fs::remove_dir_all(&previous).await;
    }

    tracing::info!(server_id = %server_id, backup_id = %info.id, "restored server backup");
    Ok(info)
}

/// Resolve an existing archive on disk without loading it into memory.
///
/// Used by the download route, which streams the file in fixed-size chunks.
pub fn archive_path(
    data_root: &str,
    server_id: &str,
    backup_id: &str,
) -> Result<PathBuf, DaemonError> {
    validate_id(backup_id)?;
    let archive = server_root(data_root, server_id)?
        .join("backups")
        .join(format!("{backup_id}.zip"));
    if !archive.is_file() {
        return Err(DaemonError::NotFound("backup not found".into()));
    }
    Ok(archive)
}

/// Recompute the archive digest and compare it with the sidecar manifest.
pub async fn verify_backup(
    data_root: &str,
    server_id: &str,
    backup_id: &str,
) -> Result<BackupVerification, DaemonError> {
    let archive = archive_path(data_root, server_id, backup_id)?;
    let backups = server_root(data_root, server_id)?.join("backups");
    let info = read_sidecar(&backups, backup_id).await?;
    let (_, actual_sha256) = hash_file(&archive).await?;
    let verified = actual_sha256.eq_ignore_ascii_case(&info.sha256);
    Ok(BackupVerification {
        id: backup_id.to_string(),
        verified,
        expected_sha256: info.sha256,
        actual_sha256,
    })
}

pub async fn delete_backup(
    data_root: &str,
    server_id: &str,
    backup_id: &str,
) -> Result<(), DaemonError> {
    validate_id(backup_id)?;
    let backups = server_root(data_root, server_id)?.join("backups");
    let archive = backups.join(format!("{backup_id}.zip"));
    if !archive.is_file() {
        return Err(DaemonError::NotFound("backup not found".into()));
    }
    tokio::fs::remove_file(&archive)
        .await
        .map_err(map_io("delete backup archive"))?;
    let _ = tokio::fs::remove_file(backups.join(format!("{backup_id}.json"))).await;
    Ok(())
}

fn server_root(data_root: &str, server_id: &str) -> Result<PathBuf, DaemonError> {
    uuid::Uuid::parse_str(server_id)
        .map_err(|_| DaemonError::BadRequest("server id must be a UUID".into()))?;
    let root = Path::new(data_root);
    let canonical_root = std::fs::canonicalize(root).map_err(map_io("resolve MC_DATA_ROOT"))?;
    let server_root = canonical_root.join(server_id);
    std::fs::create_dir_all(&server_root)
        .map_err(|error| DaemonError::Internal(format!("create server directory: {error}")))?;
    Ok(server_root)
}

fn validate_id(id: &str) -> Result<(), DaemonError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| DaemonError::BadRequest("backup id must be a UUID".into()))
}

async fn write_sidecar(directory: &Path, info: &BackupInfo) -> Result<(), DaemonError> {
    let path = directory.join(format!("{}.json", info.id));
    let bytes = serde_json::to_vec(info)
        .map_err(|error| DaemonError::Internal(format!("serialize backup manifest: {error}")))?;
    tokio::fs::write(&path, bytes)
        .await
        .map_err(map_io("write backup manifest"))
}

async fn read_sidecar(directory: &Path, backup_id: &str) -> Result<BackupInfo, DaemonError> {
    let path = directory.join(format!("{backup_id}.json"));
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| DaemonError::NotFound("backup manifest not found".into()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| DaemonError::Internal(format!("decode backup manifest: {error}")))
}

async fn hash_file(path: &Path) -> Result<(u64, String), DaemonError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(map_io("open backup archive"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = tokio::io::AsyncReadExt::read(&mut file, &mut buffer)
            .await
            .map_err(map_io("read backup archive"))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        hasher.update(&buffer[..read]);
    }
    Ok((total, hex::encode(hasher.finalize())))
}

fn write_archive(data: &Path, archive: &Path) -> Result<(), DaemonError> {
    let file = std::fs::File::create(archive).map_err(map_io("create backup archive"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut entries = 0usize;
    let mut total = 0u64;
    let mut pending = vec![data.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(map_io("read server data directory"))? {
            let entry = entry.map_err(map_io("read server data entry"))?;
            let path = entry.path();
            let metadata =
                std::fs::symlink_metadata(&path).map_err(map_io("inspect backup path"))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            entries = entries.saturating_add(1);
            if entries > MAX_BACKUP_ENTRIES {
                return Err(DaemonError::BadRequest(
                    "server has too many files to back up".into(),
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            if metadata.len() > MAX_BACKUP_FILE_BYTES {
                return Err(DaemonError::BadRequest(format!(
                    "file is too large to back up: {}",
                    path.display()
                )));
            }
            total = total.saturating_add(metadata.len());
            if total > MAX_BACKUP_BYTES {
                return Err(DaemonError::BadRequest(
                    "server data exceeds the backup size limit".into(),
                ));
            }
            let relative = path
                .strip_prefix(data)
                .map_err(|_| DaemonError::Internal("backup path escaped data root".into()))?;
            let name = relative.to_string_lossy().replace('\\', "/");
            writer
                .start_file(name, options)
                .map_err(|error| DaemonError::Internal(format!("write ZIP entry: {error}")))?;
            let mut input =
                std::fs::File::open(&path).map_err(map_io("open backup source file"))?;
            std::io::copy(&mut input, &mut writer).map_err(map_io("write backup file"))?;
        }
    }
    writer
        .finish()
        .map_err(|error| DaemonError::Internal(format!("finalize backup archive: {error}")))?;
    Ok(())
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<(), DaemonError> {
    let file = std::fs::File::open(archive).map_err(map_io("open backup archive"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| DaemonError::BadRequest(format!("invalid backup archive: {error}")))?;

    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| DaemonError::BadRequest(format!("read backup entry: {error}")))?;
        let parts = safe_components(entry.name())?;
        if parts.is_empty() {
            continue;
        }
        let target = parts
            .iter()
            .fold(destination.to_path_buf(), |path, part| path.join(part));
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(map_io("create restored directory"))?;
            continue;
        }
        if entry.size() > MAX_BACKUP_FILE_BYTES {
            return Err(DaemonError::BadRequest(
                "backup contains an oversized file".into(),
            ));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_BACKUP_BYTES {
            return Err(DaemonError::BadRequest(
                "backup exceeds the restore size limit".into(),
            ));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(map_io("create restored directory"))?;
        }
        let mut output = std::fs::File::create(&target).map_err(map_io("write restored file"))?;
        let mut limited = entry.take(MAX_BACKUP_FILE_BYTES + 1);
        let written =
            std::io::copy(&mut limited, &mut output).map_err(map_io("write restored file"))?;
        if written > MAX_BACKUP_FILE_BYTES {
            return Err(DaemonError::BadRequest(
                "backup file exceeded the restore size limit".into(),
            ));
        }
    }
    Ok(())
}

fn safe_components(name: &str) -> Result<Vec<String>, DaemonError> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains('\0') {
        return Err(DaemonError::BadRequest(
            "backup entry has an invalid path".into(),
        ));
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(DaemonError::BadRequest(
                "backup entry escapes the restore directory".into(),
            ));
        }
        if part.chars().count() > 255 || part.chars().any(char::is_control) {
            return Err(DaemonError::BadRequest(
                "backup entry has an invalid name".into(),
            ));
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn default_label() -> String {
    "Automatic backup".to_string()
}

fn map_io(context: &'static str) -> impl FnOnce(std::io::Error) -> DaemonError + Copy {
    move |error| DaemonError::Internal(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn rejects_unsafe_restore_paths() {
        assert!(safe_components("../escape").is_err());
        assert!(safe_components("/etc/passwd").is_err());
        assert!(safe_components("world/../secret").is_err());
        assert_eq!(
            safe_components("world/region/r.0.0.mca").expect("safe"),
            vec!["world", "region", "r.0.0.mca"]
        );
    }

    #[test]
    fn creates_and_restores_a_snapshot() {
        let root = std::env::temp_dir().join(format!("mc-backup-test-{}", uuid::Uuid::now_v7()));
        let id = uuid::Uuid::now_v7().to_string();
        let data = root.join(&id).join("data");
        std::fs::create_dir_all(data.join("world")).expect("data dir");
        std::fs::write(data.join("server.properties"), "motd=hello\n").expect("properties");
        std::fs::write(data.join("world/level.dat"), b"level").expect("level");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let info = runtime
            .block_on(create_backup(
                root.to_str().expect("path"),
                &id,
                None,
                false,
            ))
            .expect("backup");
        assert!(info.size_bytes > 0);
        assert_eq!(info.sha256.len(), 64);

        std::fs::write(data.join("server.properties"), "motd=changed\n").expect("mutate");
        runtime
            .block_on(restore_backup(root.to_str().expect("path"), &id, &info.id))
            .expect("restore");
        let restored = std::fs::read_to_string(data.join("server.properties")).expect("read");
        assert_eq!(restored, "motd=hello\n");
        assert!(data.join("world/level.dat").is_file());

        runtime
            .block_on(delete_backup(root.to_str().expect("path"), &id, &info.id))
            .expect("delete");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verifies_archive_integrity() {
        let root = std::env::temp_dir().join(format!("mc-verify-test-{}", uuid::Uuid::now_v7()));
        let id = uuid::Uuid::now_v7().to_string();
        let data = root.join(&id).join("data");
        std::fs::create_dir_all(&data).expect("data dir");
        std::fs::write(data.join("server.properties"), "motd=verify\n").expect("properties");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let info = runtime
            .block_on(create_backup(
                root.to_str().expect("path"),
                &id,
                None,
                false,
            ))
            .expect("backup");

        let healthy = runtime
            .block_on(verify_backup(root.to_str().expect("path"), &id, &info.id))
            .expect("verify");
        assert!(healthy.verified);
        assert_eq!(healthy.actual_sha256, healthy.expected_sha256);

        // Append a byte: the archive is now corrupt relative to its manifest.
        let archive = root
            .join(&id)
            .join("backups")
            .join(format!("{}.zip", info.id));
        std::fs::OpenOptions::new()
            .append(true)
            .open(&archive)
            .expect("open archive")
            .write_all(b"tamper")
            .expect("tamper");

        let corrupt = runtime
            .block_on(verify_backup(root.to_str().expect("path"), &id, &info.id))
            .expect("verify");
        assert!(!corrupt.verified);
        assert_ne!(corrupt.actual_sha256, corrupt.expected_sha256);

        let _ = std::fs::remove_dir_all(root);
    }
}
