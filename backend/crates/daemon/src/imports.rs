//! Import existing Minecraft server data from a ZIP archive or a host path.
//!
//! Imports always land in `<MC_DATA_ROOT>/<server_id>/data`. The destination
//! must be empty (the container-creation flow has already made the directory),
//! and every path is validated so an archive or a symlink cannot escape it.
//! ZIP extraction runs in a blocking worker; local copies stream one file at a
//! time. Failures clear the partially imported data so a retry starts clean.

use std::io::Read;
use std::path::{Path, PathBuf};

use cluster_shared::ImportServerResult;
use zip::ZipArchive;

use crate::error::DaemonError;

/// Multipart body limit for the daemon import route (10 GiB).
pub const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024 * 1024;
/// Hard cap on extracted/copied bytes, guarding against archive bombs.
const MAX_IMPORTED_BYTES: u64 = 250 * 1024 * 1024 * 1024;
/// One file inside an imported server.
const MAX_IMPORT_FILE_BYTES: u64 = 100 * 1024 * 1024 * 1024;
/// Entry count cap for a recursive local copy or ZIP extraction.
const MAX_IMPORT_ENTRIES: usize = 2_000_000;
/// Maximum characters in a host path supplied by the operator.
const MAX_LOCAL_PATH_CHARS: usize = 4096;

/// Top-level names that identify a directory as a Minecraft server root. These
/// let us strip the common `my-server/...` wrapper folder from a ZIP without
/// accidentally flattening a bare `world/` archive.
const SERVER_MARKERS: [&str; 17] = [
    "server.properties",
    "server.jar",
    "paper.jar",
    "fabric-server-launch.jar",
    "forge-installer.jar",
    "eula.txt",
    "ops.json",
    "whitelist.json",
    "banned-players.json",
    "usercache.json",
    "bukkit.yml",
    "spigot.yml",
    "paper-global.yml",
    "plugins",
    "mods",
    "libraries",
    "config",
];

/// Extract an uploaded `.zip` into the server's empty data directory.
pub async fn import_zip(
    data_root: &str,
    server_id: &str,
    archive: &Path,
) -> Result<ImportServerResult, DaemonError> {
    let destination = prepare_destination(data_root, server_id).await?;
    let archive_path = archive.to_path_buf();
    let target = destination.clone();

    let result =
        tokio::task::spawn_blocking(move || extract_zip_blocking(&archive_path, &target)).await;

    match result {
        Ok(Ok((files, bytes))) => Ok(ImportServerResult {
            id: server_id.to_string(),
            files,
            bytes,
        }),
        Ok(Err(error)) => {
            clear_directory(&destination).await?;
            Err(error)
        }
        Err(error) => {
            clear_directory(&destination).await?;
            Err(DaemonError::Internal(format!(
                "import extraction task failed: {error}"
            )))
        }
    }
}

/// Recursively copy an absolute host directory into a new server volume.
pub async fn import_local(
    data_root: &str,
    server_id: &str,
    raw_path: &str,
) -> Result<ImportServerResult, DaemonError> {
    let source = validate_local_source(raw_path)?;
    let destination = prepare_destination(data_root, server_id).await?;

    if destination.starts_with(&source) || source.starts_with(&destination) {
        clear_directory(&destination).await?;
        return Err(DaemonError::BadRequest(
            "local path must not contain the server data directory".into(),
        ));
    }

    match copy_tree(&source, &destination).await {
        Ok((files, bytes)) => Ok(ImportServerResult {
            id: server_id.to_string(),
            files,
            bytes,
        }),
        Err(error) => {
            clear_directory(&destination).await?;
            Err(error)
        }
    }
}

/// Create `<data_root>/<id>/data` if needed and require it to be empty.
async fn prepare_destination(data_root: &str, server_id: &str) -> Result<PathBuf, DaemonError> {
    uuid::Uuid::parse_str(server_id)
        .map_err(|_| DaemonError::BadRequest("server id must be a UUID".into()))?;

    let root = Path::new(data_root);
    let server_root = root.join(server_id);
    tokio::fs::create_dir_all(&server_root)
        .await
        .map_err(map_io("create server directory"))?;

    let destination = server_root.join("data");
    match tokio::fs::symlink_metadata(&destination).await {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(DaemonError::Conflict(
                    "server data path is not a directory".into(),
                ));
            }
            let mut entries = tokio::fs::read_dir(&destination)
                .await
                .map_err(map_io("inspect server data directory"))?;
            if entries
                .next_entry()
                .await
                .map_err(map_io("inspect server data directory"))?
                .is_some()
            {
                return Err(DaemonError::Conflict(
                    "server data directory is not empty; refusing to overwrite it".into(),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(&destination)
                .await
                .map_err(map_io("create server data directory"))?;
        }
        Err(error) => return Err(map_io("inspect server data directory")(error)),
    }

    let canonical_root = std::fs::canonicalize(root).map_err(map_io("resolve MC_DATA_ROOT"))?;
    let canonical_destination =
        std::fs::canonicalize(&destination).map_err(map_io("resolve server data directory"))?;
    if !canonical_destination.starts_with(&canonical_root) {
        return Err(DaemonError::Internal(
            "server data directory escaped MC_DATA_ROOT".into(),
        ));
    }
    Ok(canonical_destination)
}

fn validate_local_source(raw_path: &str) -> Result<PathBuf, DaemonError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(DaemonError::BadRequest(
            "local_path must not be empty".into(),
        ));
    }
    if trimmed.chars().count() > MAX_LOCAL_PATH_CHARS {
        return Err(DaemonError::BadRequest(format!(
            "local_path must be at most {MAX_LOCAL_PATH_CHARS} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(DaemonError::BadRequest(
            "local_path must not contain control characters".into(),
        ));
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(DaemonError::BadRequest(
            "local_path must be an absolute path on the daemon host".into(),
        ));
    }

    let canonical = std::fs::canonicalize(&path).map_err(|error| {
        DaemonError::BadRequest(format!("local_path is not accessible: {error}"))
    })?;
    let metadata = std::fs::metadata(&canonical).map_err(|error| {
        DaemonError::BadRequest(format!("local_path is not accessible: {error}"))
    })?;
    if !metadata.is_dir() {
        return Err(DaemonError::BadRequest(
            "local_path must point at a directory".into(),
        ));
    }

    // Importing the host root or a virtual filesystem would copy the machine
    // itself. Real server folders live under a normal directory.
    for forbidden in ["/", "/proc", "/sys", "/dev", "/boot", "/run"] {
        if canonical == Path::new(forbidden) {
            return Err(DaemonError::BadRequest(format!(
                "local_path must not be {forbidden}"
            )));
        }
    }

    Ok(canonical)
}

async fn copy_tree(source: &Path, destination: &Path) -> Result<(usize, u64), DaemonError> {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut entries = 0usize;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];

    while let Some((from, to)) = pending.pop() {
        let metadata = tokio::fs::symlink_metadata(&from)
            .await
            .map_err(map_io("inspect local path"))?;
        if metadata.file_type().is_symlink() {
            return Err(DaemonError::BadRequest(format!(
                "local_path contains a symlink: {}",
                from.display()
            )));
        }

        if metadata.is_dir() {
            tokio::fs::create_dir_all(&to)
                .await
                .map_err(map_io("create imported directory"))?;
            let mut read_dir = tokio::fs::read_dir(&from)
                .await
                .map_err(map_io("read local directory"))?;
            while let Some(entry) = read_dir
                .next_entry()
                .await
                .map_err(map_io("read local directory"))?
            {
                entries = entries.saturating_add(1);
                if entries > MAX_IMPORT_ENTRIES {
                    return Err(DaemonError::BadRequest(
                        "local_path contains too many entries".into(),
                    ));
                }
                pending.push((entry.path(), to.join(entry.file_name())));
            }
            continue;
        }

        if !metadata.is_file() {
            return Err(DaemonError::BadRequest(format!(
                "local_path contains a non-regular file: {}",
                from.display()
            )));
        }

        if metadata.len() > MAX_IMPORT_FILE_BYTES {
            return Err(DaemonError::BadRequest(format!(
                "local file is larger than the {} byte limit: {}",
                MAX_IMPORT_FILE_BYTES,
                from.display()
            )));
        }
        bytes = bytes.saturating_add(metadata.len());
        if bytes > MAX_IMPORTED_BYTES {
            return Err(DaemonError::BadRequest(
                "local_path exceeds the total import size limit".into(),
            ));
        }

        if tokio::fs::symlink_metadata(&to).await.is_ok() {
            return Err(DaemonError::Conflict(format!(
                "duplicate destination while importing: {}",
                to.display()
            )));
        }
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(map_io("create imported directory"))?;
        }
        tokio::fs::copy(&from, &to)
            .await
            .map_err(map_io("copy local file"))?;
        files = files.saturating_add(1);
    }

    Ok((files, bytes))
}

fn extract_zip_blocking(
    archive_path: &Path,
    destination: &Path,
) -> Result<(usize, u64), DaemonError> {
    let file = std::fs::File::open(archive_path).map_err(map_io("open import archive"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| DaemonError::BadRequest(format!("invalid ZIP archive: {error}")))?;

    let names = archive
        .file_names()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let prefix = wrapper_prefix(&names)?;

    let mut files = 0usize;
    let mut bytes = 0u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| DaemonError::BadRequest(format!("read ZIP entry: {error}")))?;
        let raw_name = entry.name().to_string();
        let mut parts = zip_components(&raw_name)?;
        if let Some(prefix) = &prefix {
            if parts.first() != Some(prefix) {
                continue;
            }
            parts.remove(0);
        }
        if parts.is_empty() {
            continue;
        }

        let is_dir = entry.is_dir() || raw_name.ends_with('/');
        let target = parts
            .iter()
            .fold(destination.to_path_buf(), |path, part| path.join(part));

        if is_dir {
            std::fs::create_dir_all(&target).map_err(map_io("create imported directory"))?;
            continue;
        }

        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(DaemonError::BadRequest(format!(
                "ZIP contains a symlink: {raw_name}"
            )));
        }
        if entry.size() > MAX_IMPORT_FILE_BYTES {
            return Err(DaemonError::BadRequest(format!(
                "ZIP file is larger than the {} byte limit: {raw_name}",
                MAX_IMPORT_FILE_BYTES
            )));
        }
        bytes = bytes.saturating_add(entry.size());
        if bytes > MAX_IMPORTED_BYTES {
            return Err(DaemonError::BadRequest(
                "ZIP exceeds the total import size limit".into(),
            ));
        }
        if files >= MAX_IMPORT_ENTRIES {
            return Err(DaemonError::BadRequest(
                "ZIP contains too many files".into(),
            ));
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(map_io("create imported directory"))?;
        }
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(DaemonError::Conflict(format!(
                "ZIP contains a duplicate path: {raw_name}"
            )));
        }

        let mut output = std::fs::File::create(&target).map_err(map_io("write imported file"))?;
        let mut limited = entry.take(MAX_IMPORT_FILE_BYTES + 1);
        let written =
            std::io::copy(&mut limited, &mut output).map_err(map_io("write imported file"))?;
        if written > MAX_IMPORT_FILE_BYTES {
            return Err(DaemonError::BadRequest(format!(
                "ZIP file exceeded the {} byte limit: {raw_name}",
                MAX_IMPORT_FILE_BYTES
            )));
        }
        files = files.saturating_add(1);
    }

    Ok((files, bytes))
}

/// Detect the common `my-server/...` wrapper folder. Returns `Some(name)` only
/// when every entry lives under one top-level directory, there are no root
/// files, and that directory contains a recognisable server marker.
fn wrapper_prefix(names: &[String]) -> Result<Option<String>, DaemonError> {
    let mut top_level: Option<String> = None;
    let mut root_file = false;
    let mut marker = false;

    for name in names {
        let parts = zip_components(name)?;
        if parts.is_empty() {
            continue;
        }
        if parts.len() == 1 {
            if !name.ends_with('/') {
                root_file = true;
            }
            continue;
        }

        let top = &parts[0];
        match &top_level {
            None => top_level = Some(top.clone()),
            Some(existing) if existing != top => {
                root_file = true;
            }
            Some(_) => {}
        }
        if SERVER_MARKERS.contains(&parts[1].as_str()) {
            marker = true;
        }
    }

    if root_file || !marker || top_level.is_none() {
        return Ok(None);
    }
    Ok(top_level)
}

/// Split a ZIP entry into validated relative components. Backslashes are
/// normalised, absolute paths and `..` are rejected, and empty segments are
/// ignored.
fn zip_components(raw_name: &str) -> Result<Vec<String>, DaemonError> {
    let name = raw_name.replace('\\', "/");
    if name.starts_with('/') || name.contains('\0') {
        return Err(DaemonError::BadRequest(format!(
            "ZIP entry has an invalid path: {raw_name}"
        )));
    }

    let mut parts = Vec::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(DaemonError::BadRequest(format!(
                "ZIP entry escapes the destination: {raw_name}"
            )));
        }
        if part.chars().count() > 255 || part.chars().any(char::is_control) {
            return Err(DaemonError::BadRequest(format!(
                "ZIP entry has an invalid name: {raw_name}"
            )));
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

async fn clear_directory(directory: &Path) -> Result<(), DaemonError> {
    let mut entries = match tokio::fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(map_io("list server data directory")(error)),
    };

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(map_io("list server data directory"))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(map_io("inspect server data entry"))?;
        if file_type.is_dir() {
            tokio::fs::remove_dir_all(entry.path())
                .await
                .map_err(map_io("remove imported directory"))?;
        } else {
            tokio::fs::remove_file(entry.path())
                .await
                .map_err(map_io("remove imported file"))?;
        }
    }
    Ok(())
}

fn map_io(context: &'static str) -> impl FnOnce(std::io::Error) -> DaemonError + Copy {
    move |error| DaemonError::Internal(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_absolute_zip_paths() {
        assert!(zip_components("../escape").is_err());
        assert!(zip_components("world/../../escape").is_err());
        assert!(zip_components("/etc/passwd").is_err());
        assert!(zip_components("C:\\Windows\\system32").is_ok());
    }

    #[test]
    fn normalises_backslashes_and_skips_empty_segments() {
        assert_eq!(
            zip_components("world//region\\\\r.0.0.mca").expect("valid"),
            vec!["world", "region", "r.0.0.mca"]
        );
    }

    #[test]
    fn detects_a_single_wrapper_folder() {
        let names = vec![
            "survival/server.properties".to_string(),
            "survival/plugins/Example.jar".to_string(),
        ];
        assert_eq!(
            wrapper_prefix(&names).expect("valid"),
            Some("survival".into())
        );
    }

    #[test]
    fn does_not_strip_a_bare_world_archive() {
        let names = vec![
            "world/level.dat".to_string(),
            "world/region/r.0.0.mca".to_string(),
        ];
        assert_eq!(wrapper_prefix(&names).expect("valid"), None);
    }

    #[test]
    fn does_not_strip_when_root_files_exist() {
        let names = vec![
            "server.properties".to_string(),
            "survival/server.properties".to_string(),
        ];
        assert_eq!(wrapper_prefix(&names).expect("valid"), None);
    }
}
