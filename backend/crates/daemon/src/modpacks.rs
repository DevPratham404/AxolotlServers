//! Modrinth `.mrpack` installation.
//!
//! A pack is a zip with `modrinth.index.json` and an `overrides/` tree. The
//! engine streams the archive to a temporary file, downloads the index's
//! server-side files concurrently (bounded parallelism), applies overrides on
//! top of the server root, and always removes its temporary artefacts.
//!
//! Every remote file URL goes through the same HTTPS/public-host validation as
//! the single-file downloader. Redirects are disabled.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use tokio::task::JoinSet;
use zip::ZipArchive;

use cluster_shared::ModpackInstallResult;

use crate::{error::DaemonError, files};

/// Simultaneous mod downloads. High enough to be quick, low enough to keep
/// file descriptors and Modrinth/CDN connections predictable.
const DOWNLOAD_CONCURRENCY: usize = 12;
/// The pack archive itself.
const MAX_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;
/// One file listed in `modrinth.index.json`.
const MAX_PACK_FILE_BYTES: u64 = 100 * 1024 * 1024;
/// `modrinth.index.json` is tiny; this stops zip bombs.
const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
/// One override file.
const MAX_OVERRIDE_FILE_BYTES: u64 = 100 * 1024 * 1024;
/// All override files combined.
const MAX_OVERRIDE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_OVERRIDE_ENTRIES: usize = 20_000;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Deserialize)]
struct PackIndex {
    #[serde(default)]
    files: Vec<PackFile>,
}

#[derive(Debug, Deserialize)]
struct PackFile {
    path: String,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    env: Option<PackEnv>,
}

#[derive(Debug, Deserialize)]
struct PackEnv {
    #[serde(default)]
    server: Option<String>,
}

struct PackDownload {
    filename: String,
    url: reqwest::Url,
}

/// Install a `.mrpack` into an existing server volume.
pub async fn install_modpack(
    data_root: &str,
    server_id: &str,
    raw_url: &str,
) -> Result<ModpackInstallResult, DaemonError> {
    let url = files::validate_download_url(raw_url)?;
    let name = pack_filename(&url)?;
    let root = files::canonical_root(data_root, server_id)?;
    let temporary = root.join(format!(".modpack-{}.mrpack", uuid::Uuid::now_v7()));

    let result = install_inner(&root, &url, &name, &temporary).await;
    let _ = tokio::fs::remove_file(&temporary).await;
    result
}

async fn install_inner(
    root: &Path,
    url: &reqwest::Url,
    name: &str,
    temporary: &Path,
) -> Result<ModpackInstallResult, DaemonError> {
    let client = files::build_download_client(DOWNLOAD_TIMEOUT)?;
    let archive_bytes = files::fetch_to_file(&client, url, temporary, MAX_ARCHIVE_BYTES).await?;

    let index = read_index(temporary).await?;
    let downloads = prepare_downloads(&index)?;
    if downloads.is_empty() {
        return Err(DaemonError::BadRequest(
            "modpack index contains no server-side files".into(),
        ));
    }

    let (mods, _) = files::prepare_directory(root, "mods").await?;
    let (files, bytes) = download_all(&client, &mods, downloads).await?;

    let staging = root.join(format!(".modpack-overrides-{}", uuid::Uuid::now_v7()));
    let overrides = async {
        let extracted = extract_overrides(temporary, &staging).await?;
        if extracted == 0 {
            return Ok(0);
        }
        apply_overrides(&staging, root, "").await
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    let overrides = overrides?;

    tracing::info!(
        archive_bytes = %archive_bytes,
        files = %files,
        bytes = %bytes,
        overrides = %overrides,
        "installed modpack"
    );

    Ok(ModpackInstallResult {
        name: name.to_string(),
        files,
        overrides,
        bytes,
    })
}

/// Parse `modrinth.index.json` from the archive in a blocking worker.
async fn read_index(temporary: &Path) -> Result<PackIndex, DaemonError> {
    let path = temporary.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).map_err(files::map_io("open modpack archive"))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|error| DaemonError::BadRequest(format!("invalid modpack zip: {error}")))?;
        let entry = archive
            .by_name("modrinth.index.json")
            .map_err(|_| DaemonError::BadRequest("modpack has no modrinth.index.json".into()))?;
        if entry.size() > MAX_INDEX_BYTES {
            return Err(DaemonError::BadRequest(
                "modrinth.index.json is unreasonably large".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .take(MAX_INDEX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(files::map_io("read modpack index"))?;
        if bytes.len() as u64 > MAX_INDEX_BYTES {
            return Err(DaemonError::BadRequest(
                "modrinth.index.json is unreasonably large".into(),
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| DaemonError::BadRequest(format!("invalid modpack index: {error}")))
    })
    .await
    .map_err(|error| DaemonError::Internal(format!("modpack index task failed: {error}")))?
}

/// Keep server-compatible files, validate every URL, and flatten index paths to
/// a unique filename destined for `mods/`.
fn prepare_downloads(index: &PackIndex) -> Result<Vec<PackDownload>, DaemonError> {
    let mut seen = HashSet::new();
    let mut downloads = Vec::new();

    for entry in &index.files {
        if entry
            .env
            .as_ref()
            .and_then(|env| env.server.as_deref())
            .is_some_and(|value| value.eq_ignore_ascii_case("unsupported"))
        {
            continue;
        }

        let source = entry.downloads.first().ok_or_else(|| {
            DaemonError::BadRequest(format!("modpack file {} has no download URL", entry.path))
        })?;
        let url = files::validate_download_url(source)?;
        let filename = pack_destination_name(&entry.path)?;
        if !seen.insert(filename.to_ascii_lowercase()) {
            return Err(DaemonError::BadRequest(format!(
                "modpack lists duplicate file name: {filename}"
            )));
        }
        downloads.push(PackDownload { filename, url });
    }

    Ok(downloads)
}

/// Take the final path component and validate it as a plain filename.
fn pack_destination_name(path: &str) -> Result<String, DaemonError> {
    let (parts, _) = files::relative_components(path)?;
    let name = parts
        .last()
        .ok_or_else(|| DaemonError::BadRequest("modpack file path is empty".into()))?;
    files::validate_download_filename(name)
}

async fn download_all(
    client: &reqwest::Client,
    directory: &Path,
    downloads: Vec<PackDownload>,
) -> Result<(usize, u64), DaemonError> {
    let mut pending = downloads.into_iter();
    let mut tasks: JoinSet<Result<u64, DaemonError>> = JoinSet::new();

    while tasks.len() < DOWNLOAD_CONCURRENCY {
        let Some(download) = pending.next() else {
            break;
        };
        spawn_download(&mut tasks, client, directory, download);
    }

    let mut files = 0usize;
    let mut bytes = 0u64;
    while let Some(joined) = tasks.join_next().await {
        let downloaded = joined.map_err(|error| {
            DaemonError::Internal(format!("modpack download task failed: {error}"))
        })?;
        bytes = bytes.saturating_add(downloaded?);
        files += 1;
        if let Some(download) = pending.next() {
            spawn_download(&mut tasks, client, directory, download);
        }
    }

    Ok((files, bytes))
}

fn spawn_download(
    tasks: &mut JoinSet<Result<u64, DaemonError>>,
    client: &reqwest::Client,
    directory: &Path,
    download: PackDownload,
) {
    let client = client.clone();
    let directory = directory.to_path_buf();
    tasks.spawn(async move {
        let destination = directory.join(&download.filename);
        match tokio::fs::symlink_metadata(&destination).await {
            Ok(metadata) if metadata.is_dir() => {
                return Err(DaemonError::Conflict(format!(
                    "mod destination is a directory: {}",
                    download.filename
                )));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DaemonError::BadRequest(format!(
                    "refusing to replace a symlink: {}",
                    download.filename
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(files::map_io("inspect mod destination")(error)),
        }

        let temporary = directory.join(format!(
            ".{}.download-{}",
            download.filename,
            uuid::Uuid::now_v7()
        ));
        let written =
            files::fetch_to_file(&client, &download.url, &temporary, MAX_PACK_FILE_BYTES).await;

        let bytes = match written {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error);
            }
        };

        if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(files::map_io("replace mod file")(error));
        }
        Ok(bytes)
    });
}

/// Extract `overrides/` into a staging directory inside the volume. Paths are
/// validated before any write, symlink entries are rejected, and sizes are
/// capped so a hostile pack cannot fill the disk or escape the root.
async fn extract_overrides(temporary: &Path, staging: &Path) -> Result<usize, DaemonError> {
    let archive = temporary.to_path_buf();
    let target = staging.to_path_buf();
    tokio::task::spawn_blocking(move || extract_overrides_blocking(&archive, &target))
        .await
        .map_err(|error| DaemonError::Internal(format!("modpack override task failed: {error}")))?
}

fn extract_overrides_blocking(archive_path: &Path, staging: &Path) -> Result<usize, DaemonError> {
    let file = std::fs::File::open(archive_path).map_err(files::map_io("open modpack archive"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| DaemonError::BadRequest(format!("invalid modpack zip: {error}")))?;

    std::fs::create_dir_all(staging).map_err(files::map_io("create override staging"))?;

    let mut written = 0usize;
    let mut total = 0u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| DaemonError::BadRequest(format!("read modpack entry: {error}")))?;
        if written >= MAX_OVERRIDE_ENTRIES {
            return Err(DaemonError::BadRequest(
                "modpack overrides contain too many entries".into(),
            ));
        }

        let name = entry.name().to_string();
        let Some(relative) = name.strip_prefix("overrides/") else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(DaemonError::BadRequest(format!(
                "modpack override is a symlink: {relative}"
            )));
        }

        let (parts, _) = files::relative_components(relative)?;
        let destination = parts
            .iter()
            .fold(staging.to_path_buf(), |path, part| path.join(part));

        if entry.is_dir() || name.ends_with('/') {
            std::fs::create_dir_all(&destination)
                .map_err(files::map_io("create override directory"))?;
            continue;
        }

        if entry.size() > MAX_OVERRIDE_FILE_BYTES {
            return Err(DaemonError::BadRequest(format!(
                "modpack override is too large: {relative}"
            )));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_OVERRIDE_TOTAL_BYTES {
            return Err(DaemonError::BadRequest(
                "modpack overrides exceed the total size limit".into(),
            ));
        }

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(files::map_io("create override directory"))?;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .take(MAX_OVERRIDE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(files::map_io("read modpack override"))?;
        if bytes.len() as u64 > MAX_OVERRIDE_FILE_BYTES {
            return Err(DaemonError::BadRequest(format!(
                "modpack override is too large: {relative}"
            )));
        }
        std::fs::write(&destination, bytes).map_err(files::map_io("write modpack override"))?;
        written += 1;
    }

    Ok(written)
}

/// Move staged override files into the server root, creating parents safely and
/// replacing existing regular files. Symlink destinations are never followed.
async fn apply_overrides(staging: &Path, root: &Path, prefix: &str) -> Result<usize, DaemonError> {
    let mut read_dir = tokio::fs::read_dir(staging)
        .await
        .map_err(files::map_io("list override staging"))?;
    let mut applied = 0usize;

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(files::map_io("read override staging entry"))?
    {
        let name = entry
            .file_name()
            .to_str()
            .map(ToString::to_string)
            .ok_or_else(|| DaemonError::BadRequest("override name is not UTF-8".into()))?;
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry
            .file_type()
            .await
            .map_err(files::map_io("inspect override staging entry"))?;

        if file_type.is_dir() {
            files::prepare_directory(root, &relative).await?;
            applied += Box::pin(apply_overrides(&entry.path(), root, &relative)).await?;
            continue;
        }
        if !file_type.is_file() {
            return Err(DaemonError::BadRequest(format!(
                "modpack override is not a regular file: {relative}"
            )));
        }

        let (parent, _) = files::prepare_directory(root, prefix).await?;
        let destination = parent.join(&name);
        match tokio::fs::symlink_metadata(&destination).await {
            Ok(metadata) if metadata.is_dir() => {
                return Err(DaemonError::Conflict(format!(
                    "override destination is a directory: {relative}"
                )));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DaemonError::BadRequest(format!(
                    "refusing to replace a symlink: {relative}"
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(files::map_io("inspect override destination")(error)),
        }

        tokio::fs::rename(entry.path(), &destination)
            .await
            .map_err(files::map_io("apply modpack override"))?;
        applied += 1;
    }

    Ok(applied)
}

fn pack_filename(url: &reqwest::Url) -> Result<String, DaemonError> {
    let name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| DaemonError::BadRequest("modpack URL has no filename".into()))?;
    let name = files::validate_download_filename(name)?;
    if !name.to_ascii_lowercase().ends_with(".mrpack") {
        return Err(DaemonError::BadRequest(
            "modpack URL must point at a .mrpack file".into(),
        ));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn sample_index() -> &'static str {
        r#"{
            "files": [
                {"path": "mods/a.jar", "downloads": ["https://cdn.modrinth.com/a.jar"]},
                {"path": "mods/b.jar", "downloads": ["https://cdn.modrinth.com/b.jar"],
                 "env": {"client": "required", "server": "unsupported"}},
                {"path": "mods/c.jar", "downloads": ["https://cdn.modrinth.com/c.jar"],
                 "env": {"client": "optional", "server": "optional"}}
            ]
        }"#
    }

    #[test]
    fn filters_server_unsupported_pack_files() {
        let index: PackIndex = serde_json::from_str(sample_index()).expect("valid index");
        let downloads = prepare_downloads(&index).expect("valid downloads");
        assert_eq!(downloads.len(), 2);
        assert_eq!(downloads[0].filename, "a.jar");
        assert_eq!(downloads[1].filename, "c.jar");
    }

    #[test]
    fn rejects_traversal_and_duplicate_pack_paths() {
        assert!(pack_destination_name("../escape.jar").is_err());
        assert!(pack_destination_name("mods/nested/a.jar").is_ok());
        let index: PackIndex = serde_json::from_str(
            r#"{"files":[
                {"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"]},
                {"path":"other/a.jar","downloads":["https://cdn.modrinth.com/b.jar"]}
            ]}"#,
        )
        .expect("valid index");
        assert!(prepare_downloads(&index).is_err());
    }

    #[test]
    fn rejects_non_https_pack_downloads() {
        let index: PackIndex = serde_json::from_str(
            r#"{"files":[{"path":"mods/a.jar","downloads":["http://cdn.modrinth.com/a.jar"]}]}"#,
        )
        .expect("valid index");
        assert!(prepare_downloads(&index).is_err());
    }

    #[test]
    fn pack_filename_requires_mrpack() {
        let url = reqwest::Url::parse("https://cdn.modrinth.com/data/a/versions/b/pack.mrpack")
            .expect("url");
        assert_eq!(pack_filename(&url).expect("name"), "pack.mrpack");
        let wrong = reqwest::Url::parse("https://cdn.modrinth.com/data/a/versions/b/pack.zip")
            .expect("url");
        assert!(pack_filename(&wrong).is_err());
    }

    #[test]
    fn extracts_only_overrides_and_rejects_traversal() {
        let root = std::env::temp_dir().join(format!("cluster-mrpack-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("create root");
        let archive_path = root.join("pack.mrpack");
        let staging = root.join("staging");

        {
            let file = std::fs::File::create(&archive_path).expect("create archive");
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file::<_, ()>("overrides/config/test.yml", SimpleFileOptions::default())
                .expect("start override");
            use std::io::Write;
            writer.write_all(b"enabled: true").expect("write override");
            writer
                .start_file::<_, ()>("modrinth.index.json", SimpleFileOptions::default())
                .expect("start index");
            writer.write_all(b"{}").expect("write index");
            writer.finish().expect("finish archive");
        }

        let count = extract_overrides_blocking(&archive_path, &staging).expect("extract");
        assert_eq!(count, 1);
        assert_eq!(
            std::fs::read_to_string(staging.join("config/test.yml")).expect("read"),
            "enabled: true"
        );
        assert!(!staging.join("modrinth.index.json").exists());

        let _ = std::fs::remove_dir_all(root);
    }
}
