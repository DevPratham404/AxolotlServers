//! Sandboxed read/write access to a server's `/data` volume.
//!
//! Security is the whole point of this module: every path is validated
//! component-by-component (no `..`, no absolute paths, no NUL bytes) and then
//! canonicalised. The canonical target must remain under the canonical server
//! root, which also blocks symlink escapes. Writes are atomic
//! (`temp file → fsync → rename`) and bounded in size.

use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use cluster_shared::{
    DirectoryEntry, FileContent, FileWriteRequest, PlayerAccessLists, PlayerAccessUpdate,
    PlayerHistoryEntry,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::DaemonError;

const OPS_FILE: &str = "ops.json";
const WHITELIST_FILE: &str = "whitelist.json";
const BANNED_FILE: &str = "banned-players.json";
const USER_CACHE_FILE: &str = "usercache.json";

/// Upper bound for editable text files. Real `server.properties` files are tiny;
/// this stops the API from being used as a volume for arbitrary data.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

/// Upper bound for arbitrary binary file uploads (e.g. plugins, worlds, configs): 100 MiB.
pub const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

/// Upper bound for remote add-on downloads (Modrinth jars/zips): 50 MiB.
pub const MAX_ADDON_BYTES: usize = 50 * 1024 * 1024;

/// `<MC_DATA_ROOT>/<serverId>/data`, the container's `/data` mount.
pub fn server_data_root(data_root: &str, server_id: &str) -> PathBuf {
    Path::new(data_root).join(server_id).join("data")
}

/// Read raw bytes of a file from the sandbox, along with its filename and length.
pub async fn read_raw(
    data_root: &str,
    server_id: &str,
    relative: &str,
) -> Result<(Vec<u8>, String), DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let (path, normalised) = resolve_existing(&root, relative)?;

    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(map_io("inspect file"))?;
    if !metadata.is_file() {
        return Err(DaemonError::BadRequest("path is not a regular file".into()));
    }
    if metadata.len() > MAX_UPLOAD_BYTES as u64 {
        return Err(DaemonError::BadRequest(format!(
            "file is larger than {MAX_UPLOAD_BYTES} bytes"
        )));
    }

    let bytes = tokio::fs::read(&path).await.map_err(map_io("read file"))?;
    Ok((bytes, normalised))
}

/// Read a UTF-8 text file from the sandbox.
pub async fn read_text(
    data_root: &str,
    server_id: &str,
    relative: &str,
) -> Result<FileContent, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let (path, normalised) = resolve_existing(&root, relative)?;

    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(map_io("inspect file"))?;
    if !metadata.is_file() {
        return Err(DaemonError::BadRequest("path is not a regular file".into()));
    }
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(DaemonError::BadRequest(format!(
            "file is larger than {MAX_FILE_BYTES} bytes"
        )));
    }

    let bytes = tokio::fs::read(&path).await.map_err(map_io("read file"))?;
    let content = String::from_utf8(bytes)
        .map_err(|_| DaemonError::BadRequest("file is not valid UTF-8 text".into()))?;

    Ok(FileContent {
        path: normalised,
        hash: hash_content(&content),
        content,
    })
}

/// Atomically replace a UTF-8 text file inside the sandbox.
pub async fn write_text(
    data_root: &str,
    server_id: &str,
    relative: &str,
    request: &FileWriteRequest,
) -> Result<FileContent, DaemonError> {
    if request.content.len() > MAX_FILE_BYTES {
        return Err(DaemonError::BadRequest(format!(
            "content is larger than {MAX_FILE_BYTES} bytes"
        )));
    }

    let root = canonical_root(data_root, server_id)?;
    let (path, normalised) = resolve_for_write(&root, relative)?;

    if let Some(expected) = &request.expected_hash {
        let current = read_text(data_root, server_id, relative).await?;
        if &current.hash != expected {
            return Err(DaemonError::Conflict(
                "file changed on disk since it was loaded".into(),
            ));
        }
    }

    let parent = path
        .parent()
        .ok_or_else(|| DaemonError::BadRequest("path has no parent directory".into()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DaemonError::BadRequest("invalid file name".into()))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::now_v7()));

    let write = async {
        tokio::fs::write(&temporary, request.content.as_bytes())
            .await
            .map_err(map_io("write temporary file"))?;
        let handle = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .await
            .map_err(map_io("open temporary file"))?;
        handle
            .sync_all()
            .await
            .map_err(map_io("flush temporary file"))?;
        tokio::fs::rename(&temporary, &path)
            .await
            .map_err(map_io("replace file"))
    }
    .await;

    if write.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    write?;

    Ok(FileContent {
        path: normalised,
        hash: hash_content(&request.content),
        content: request.content.clone(),
    })
}

/// Atomically write raw binary or text bytes into the sandbox.
pub async fn write_raw(
    data_root: &str,
    server_id: &str,
    relative: &str,
    bytes: &[u8],
) -> Result<String, DaemonError> {
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(DaemonError::BadRequest(format!(
            "content is larger than {MAX_UPLOAD_BYTES} bytes"
        )));
    }

    let root = canonical_root(data_root, server_id)?;
    let (path, normalised) = resolve_for_write(&root, relative)?;

    let parent = path
        .parent()
        .ok_or_else(|| DaemonError::BadRequest("path has no parent directory".into()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DaemonError::BadRequest("invalid file name".into()))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::now_v7()));

    let write = async {
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(map_io("write temporary file"))?;
        let handle = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .await
            .map_err(map_io("open temporary file"))?;
        handle
            .sync_all()
            .await
            .map_err(map_io("flush temporary file"))?;
        tokio::fs::rename(&temporary, &path)
            .await
            .map_err(map_io("replace file"))
    }
    .await;

    if write.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    write?;

    Ok(normalised)
}

/// Create an empty directory inside the sandbox.
pub async fn create_directory(
    data_root: &str,
    server_id: &str,
    relative: &str,
) -> Result<String, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let (path, normalised) = resolve_for_write(&root, relative)?;

    if path.exists() {
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(map_io("inspect path"))?;
        if meta.is_dir() {
            return Ok(normalised);
        }
        return Err(DaemonError::Conflict(
            "a file already exists with that name".into(),
        ));
    }

    tokio::fs::create_dir_all(&path)
        .await
        .map_err(map_io("create directory"))?;
    tracing::info!(path = %normalised, "created server directory");
    Ok(normalised)
}

pub fn hash_content(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    hex::encode(digest)
}

/// Read `usercache.json`, the record of players who have joined at least once.
/// A missing file (server never started) is an empty history, not an error.
pub async fn read_player_history(
    data_root: &str,
    server_id: &str,
) -> Result<Vec<PlayerHistoryEntry>, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    history_from_root(&root).await
}

async fn history_from_root(root: &Path) -> Result<Vec<PlayerHistoryEntry>, DaemonError> {
    let path = match resolve_existing(root, USER_CACHE_FILE) {
        Ok((path, _)) => path,
        Err(DaemonError::NotFound(_)) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(map_io("read usercache"))?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(DaemonError::BadRequest(
            "usercache.json is unreasonably large".into(),
        ));
    }

    serde_json::from_slice(&bytes)
        .map_err(|error| DaemonError::BadRequest(format!("invalid usercache.json: {error}")))
}

/// Read the operator, whitelist and ban name lists.
pub async fn read_access_lists(
    data_root: &str,
    server_id: &str,
) -> Result<PlayerAccessLists, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let operators = read_list_names(&root, OPS_FILE).await?;
    let whitelisted = read_list_names(&root, WHITELIST_FILE).await?;
    let banned = read_list_names(&root, BANNED_FILE).await?;
    Ok(PlayerAccessLists {
        operators,
        whitelisted,
        banned,
    })
}

/// Apply an access change when the server is offline by editing the vanilla
/// JSON lists. Adding an entry needs the UUID recorded in `usercache.json`.
pub async fn update_player_access_offline(
    data_root: &str,
    server_id: &str,
    username: &str,
    update: PlayerAccessUpdate,
) -> Result<PlayerAccessLists, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let cache = history_from_root(&root).await?;

    let mut operators = load_list(&root, OPS_FILE).await?;
    let mut whitelisted = load_list(&root, WHITELIST_FILE).await?;
    let mut banned = load_list(&root, BANNED_FILE).await?;

    if let Some(enabled) = update.operator {
        apply_flag(&mut operators, username, enabled, &cache, "operator")?;
        save_list(&root, OPS_FILE, &operators).await?;
    }
    if let Some(enabled) = update.whitelisted {
        apply_flag(&mut whitelisted, username, enabled, &cache, "whitelist")?;
        save_list(&root, WHITELIST_FILE, &whitelisted).await?;
    }
    if let Some(enabled) = update.banned {
        apply_flag(&mut banned, username, enabled, &cache, "ban")?;
        save_list(&root, BANNED_FILE, &banned).await?;
    }

    Ok(PlayerAccessLists {
        operators: list_names(&operators),
        whitelisted: list_names(&whitelisted),
        banned: list_names(&banned),
    })
}

fn apply_flag(
    entries: &mut Vec<Value>,
    username: &str,
    enabled: bool,
    cache: &[PlayerHistoryEntry],
    kind: &str,
) -> Result<(), DaemonError> {
    if enabled {
        if entries.iter().any(|entry| name_matches(entry, username)) {
            return Ok(());
        }
        let uuid = cache
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(username))
            .map(|entry| entry.uuid.clone())
            .ok_or_else(|| {
                DaemonError::Conflict(format!(
                    "cannot add {username} to the {kind} list: no UUID in usercache.json yet"
                ))
            })?;
        let entry = match kind {
            "operator" => json!({
                "uuid": uuid,
                "name": username,
                "level": 4,
                "bypassesPlayerLimit": false,
            }),
            "ban" => json!({
                "uuid": uuid,
                "name": username,
                "created": time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
                "source": "Cluster Console",
                "expires": "forever",
                "reason": "Banned by an operator",
            }),
            _ => json!({
                "uuid": uuid,
                "name": username,
            }),
        };
        entries.push(entry);
    } else {
        entries.retain(|entry| !name_matches(entry, username));
    }
    Ok(())
}

async fn read_list_names(root: &Path, file: &str) -> Result<Vec<String>, DaemonError> {
    let entries = load_list(root, file).await?;
    Ok(list_names(&entries))
}

fn list_names(entries: &[Value]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect()
}

fn name_matches(entry: &Value, username: &str) -> bool {
    entry
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(username))
}

async fn load_list(root: &Path, file: &str) -> Result<Vec<Value>, DaemonError> {
    let path = match resolve_existing(root, file) {
        Ok((path, _)) => path,
        Err(DaemonError::NotFound(_)) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let bytes = tokio::fs::read(&path).await.map_err(map_io("read list"))?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(DaemonError::BadRequest(format!(
            "{file} is unreasonably large"
        )));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| DaemonError::BadRequest(format!("invalid {file}: {error}")))?;
    value
        .as_array()
        .cloned()
        .ok_or_else(|| DaemonError::BadRequest(format!("{file} must contain a JSON array")))
}

async fn save_list(root: &Path, file: &str, entries: &[Value]) -> Result<(), DaemonError> {
    let (path, _) = resolve_for_write(root, file)?;
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|error| DaemonError::Internal(format!("serialize {file}: {error}")))?;
    write_bytes_atomic(&path, &bytes).await
}

async fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), DaemonError> {
    let parent = path
        .parent()
        .ok_or_else(|| DaemonError::BadRequest("path has no parent directory".into()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DaemonError::BadRequest("invalid file name".into()))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::now_v7()));

    let write = async {
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(map_io("write temporary file"))?;
        tokio::fs::rename(&temporary, path)
            .await
            .map_err(map_io("replace file"))
    }
    .await;

    if write.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    write
}

/// Rename or move a file/directory inside the sandbox.
pub async fn rename_path(
    data_root: &str,
    server_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<String, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let (source, _) = resolve_existing(&root, old_path)?;
    let (destination, normalised) = resolve_for_write(&root, new_path)?;

    if source == destination {
        return Err(DaemonError::BadRequest(
            "source and destination are the same path".into(),
        ));
    }
    if destination.exists() {
        return Err(DaemonError::Conflict("destination already exists".into()));
    }

    tokio::fs::rename(&source, &destination)
        .await
        .map_err(map_io("rename path"))?;
    tracing::info!(from = %old_path, to = %normalised, "renamed server path");
    Ok(normalised)
}

/// Copy a file or directory inside the sandbox. Symlinks are never followed.
pub async fn copy_path(
    data_root: &str,
    server_id: &str,
    source_path: &str,
    dest_path: &str,
) -> Result<String, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let (source, _) = resolve_existing(&root, source_path)?;
    let (destination, normalised) = resolve_for_write(&root, dest_path)?;

    if source == destination {
        return Err(DaemonError::BadRequest(
            "source and destination are the same path".into(),
        ));
    }
    if destination.exists() {
        return Err(DaemonError::Conflict("destination already exists".into()));
    }

    let metadata = tokio::fs::symlink_metadata(&source)
        .await
        .map_err(map_io("inspect source"))?;
    if metadata.file_type().is_symlink() {
        return Err(DaemonError::BadRequest("symlinks cannot be copied".into()));
    }

    let mut copied = 0u64;
    if metadata.is_dir() {
        copy_directory(&source, &destination, &mut copied).await?;
    } else if metadata.is_file() {
        copy_file(&source, &destination, &mut copied).await?;
    } else {
        return Err(DaemonError::BadRequest(
            "path is not a regular file or directory".into(),
        ));
    }

    tracing::info!(from = %source_path, to = %normalised, bytes = %copied, "copied server path");
    Ok(normalised)
}

fn copy_directory<'a>(
    source: &'a Path,
    destination: &'a Path,
    copied: &'a mut u64,
) -> Pin<Box<dyn std::future::Future<Output = Result<(), DaemonError>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir(destination)
            .await
            .map_err(map_io("create destination directory"))?;

        let mut read_dir = tokio::fs::read_dir(source)
            .await
            .map_err(map_io("list source directory"))?;
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(map_io("read source entry"))?
        {
            let entry_path = entry.path();
            let metadata = tokio::fs::symlink_metadata(&entry_path)
                .await
                .map_err(map_io("inspect source entry"))?;
            if metadata.file_type().is_symlink() {
                return Err(DaemonError::BadRequest("symlinks cannot be copied".into()));
            }

            let target = destination.join(entry.file_name());
            if metadata.is_dir() {
                copy_directory(&entry_path, &target, copied).await?;
            } else if metadata.is_file() {
                copy_file(&entry_path, &target, copied).await?;
            } else {
                return Err(DaemonError::BadRequest(
                    "source contains a non-regular entry".into(),
                ));
            }
        }
        Ok(())
    })
}

async fn copy_file(source: &Path, destination: &Path, copied: &mut u64) -> Result<(), DaemonError> {
    let metadata = tokio::fs::metadata(source)
        .await
        .map_err(map_io("inspect source file"))?;
    if !metadata.is_file() {
        return Err(DaemonError::BadRequest("path is not a regular file".into()));
    }
    *copied = copied.saturating_add(metadata.len());
    if *copied > MAX_UPLOAD_BYTES as u64 {
        return Err(DaemonError::BadRequest(format!(
            "copy exceeds the {MAX_UPLOAD_BYTES} byte limit"
        )));
    }
    tokio::fs::copy(source, destination)
        .await
        .map_err(map_io("copy file"))?;
    Ok(())
}

/// Stream a remote file into a directory inside the sandbox.
///
/// The URL must be HTTPS and must not target a loopback/private address.
/// Redirects are disabled so a public URL cannot bounce to an internal host.
/// The body is written to a temporary file and atomically renamed on success,
/// and the total size is capped at [`MAX_ADDON_BYTES`].
pub async fn download_addon(
    data_root: &str,
    server_id: &str,
    url: &str,
    filename: &str,
    target_directory: &str,
) -> Result<(String, u64), DaemonError> {
    let url = validate_download_url(url)?;
    let filename = validate_download_filename(filename)?;
    let root = canonical_root(data_root, server_id)?;
    let (directory, normalised_dir) = prepare_directory(&root, target_directory).await?;
    let destination = directory.join(&filename);

    match tokio::fs::symlink_metadata(&destination).await {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Err(DaemonError::Conflict(
                    "destination already exists as a directory".into(),
                ));
            }
            if metadata.file_type().is_symlink() {
                return Err(DaemonError::BadRequest(
                    "refusing to replace a symlink".into(),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(map_io("inspect download destination")(error)),
    }

    let temporary = directory.join(format!(".{filename}.download-{}", uuid::Uuid::now_v7()));
    let client = build_download_client(Duration::from_secs(180))?;
    let written = fetch_to_file(&client, &url, &temporary, MAX_ADDON_BYTES as u64).await;
    let bytes = match written {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
    };

    if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(map_io("replace downloaded file")(error));
    }

    let relative = if normalised_dir.is_empty() {
        filename
    } else {
        format!("{normalised_dir}/{filename}")
    };
    tracing::info!(
        url = %url,
        path = %relative,
        bytes = %bytes,
        "downloaded add-on into server volume"
    );
    Ok((relative, bytes))
}

/// Build an HTTPS-only download client. Redirects stay disabled so a public URL
/// cannot bounce into a private address.
pub(crate) fn build_download_client(timeout: Duration) -> Result<reqwest::Client, DaemonError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .map_err(|error| DaemonError::Internal(format!("build download client: {error}")))
}

/// Open the remote URL and stream its body into `temporary`, enforcing a byte
/// cap both from `Content-Length` and while reading.
pub(crate) async fn fetch_to_file(
    client: &reqwest::Client,
    url: &reqwest::Url,
    temporary: &Path,
    max_bytes: u64,
) -> Result<u64, DaemonError> {
    let response =
        client.get(url.clone()).send().await.map_err(|error| {
            DaemonError::Unavailable(format!("download request failed: {error}"))
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(DaemonError::BadRequest(format!(
            "download source returned HTTP {status}"
        )));
    }
    if let Some(length) = response.content_length() {
        if length > max_bytes {
            return Err(DaemonError::BadRequest(format!(
                "download is larger than the {max_bytes} byte limit"
            )));
        }
    }

    let file = tokio::fs::File::create(temporary)
        .await
        .map_err(map_io("create download temporary file"))?;
    let mut writer = tokio::io::BufWriter::new(file);
    let mut stream = response.bytes_stream();
    let mut total = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|error| DaemonError::Unavailable(format!("read download stream: {error}")))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > max_bytes {
            return Err(DaemonError::BadRequest(format!(
                "download exceeds the {max_bytes} byte limit"
            )));
        }
        writer
            .write_all(&chunk)
            .await
            .map_err(map_io("write download"))?;
    }

    writer.flush().await.map_err(map_io("flush download"))?;
    writer
        .into_inner()
        .sync_all()
        .await
        .map_err(map_io("sync download"))?;

    Ok(total)
}

/// Create (or validate) a directory chain under the server root. Every existing
/// component is canonicalised and checked, so a symlinked ancestor cannot
/// redirect the download outside the volume.
pub(crate) async fn prepare_directory(
    root: &Path,
    relative: &str,
) -> Result<(PathBuf, String), DaemonError> {
    if relative.trim().is_empty() {
        return Ok((root.to_path_buf(), String::new()));
    }

    let (parts, normalised) = relative_components(relative)?;
    let mut current = root.to_path_buf();

    for part in parts {
        let next = current.join(&part);
        match tokio::fs::symlink_metadata(&next).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    let canonical = tokio::fs::canonicalize(&next)
                        .await
                        .map_err(map_io("resolve target directory"))?;
                    ensure_within(root, &canonical)?;
                    let metadata = tokio::fs::metadata(&canonical)
                        .await
                        .map_err(map_io("inspect target directory"))?;
                    if !metadata.is_dir() {
                        return Err(DaemonError::BadRequest(format!(
                            "target path component is not a directory: {part}"
                        )));
                    }
                    current = canonical;
                } else if metadata.is_dir() {
                    current = next;
                } else {
                    return Err(DaemonError::BadRequest(format!(
                        "target path component is not a directory: {part}"
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&next)
                    .await
                    .map_err(map_io("create target directory"))?;
                current = next;
            }
            Err(error) => return Err(map_io("inspect target directory")(error)),
        }
    }

    let canonical = tokio::fs::canonicalize(&current)
        .await
        .map_err(map_io("resolve target directory"))?;
    ensure_within(root, &canonical)?;
    Ok((canonical, normalised))
}

/// Accept only HTTPS URLs whose host is not loopback/private.
pub(crate) fn validate_download_url(raw: &str) -> Result<reqwest::Url, DaemonError> {
    let url = reqwest::Url::parse(raw.trim())
        .map_err(|error| DaemonError::BadRequest(format!("invalid download URL: {error}")))?;

    if url.scheme() != "https" {
        return Err(DaemonError::BadRequest(
            "download URL must use https".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(DaemonError::BadRequest(
            "download URL must not contain credentials".into(),
        ));
    }

    let host = url
        .host_str()
        .ok_or_else(|| DaemonError::BadRequest("download URL has no host".into()))?;
    let lower = host.to_ascii_lowercase();
    if lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
    {
        return Err(DaemonError::BadRequest(
            "download URL must target a public host".into(),
        ));
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let private = match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_loopback()
                    || ip.is_private()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
                    || ip.is_multicast()
            }
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_unspecified()
                    || ip.is_multicast()
            }
        };
        if private {
            return Err(DaemonError::BadRequest(
                "download URL must target a public host".into(),
            ));
        }
    }

    Ok(url)
}

pub(crate) fn validate_download_filename(raw: &str) -> Result<String, DaemonError> {
    let name = raw.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(DaemonError::BadRequest(
            "download filename must not be empty".into(),
        ));
    }
    if name.chars().count() > 200 {
        return Err(DaemonError::BadRequest(
            "download filename must be at most 200 characters".into(),
        ));
    }
    if name.contains(['/', '\\', '\0']) || name.chars().any(char::is_control) {
        return Err(DaemonError::BadRequest(
            "download filename must not contain path separators or control characters".into(),
        ));
    }
    Ok(name.to_string())
}

/// List the immediate children of a directory inside the sandbox. The empty
/// path addresses the server data root; every other path is validated exactly
/// like a file read. Symlinks that resolve outside the volume are omitted.
pub async fn list_directory(
    data_root: &str,
    server_id: &str,
    relative: &str,
) -> Result<Vec<DirectoryEntry>, DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let path = if relative.is_empty() {
        root.clone()
    } else {
        match resolve_existing(&root, relative) {
            Ok((path, _)) => path,
            // Optional add-on folders (`plugins`, `mods`, `resourcepacks`,
            // `<world>/datapacks`) do not exist until the server or an install
            // creates them. Listing them is a normal empty listing, not a 404.
            Err(DaemonError::NotFound(_)) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
    };

    let metadata = match tokio::fs::metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(map_io("inspect directory")(error)),
    };
    if !metadata.is_dir() {
        return Err(DaemonError::BadRequest("path is not a directory".into()));
    }

    let mut read_dir = tokio::fs::read_dir(&path)
        .await
        .map_err(map_io("list directory"))?;
    let mut entries = Vec::new();

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(map_io("read directory entry"))?
    {
        let Some(name) = entry.file_name().to_str().map(ToString::to_string) else {
            continue;
        };
        let entry_path = entry.path();

        let symlink_metadata = match tokio::fs::symlink_metadata(&entry_path).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        // Follow symlinks only when their canonical target stays in the volume.
        let metadata = if symlink_metadata.file_type().is_symlink() {
            let Ok(canonical) = tokio::fs::canonicalize(&entry_path).await else {
                continue;
            };
            if !canonical.starts_with(&root) {
                tracing::warn!(
                    root = %root.display(),
                    candidate = %canonical.display(),
                    "rejected directory entry escaping the server volume"
                );
                continue;
            }
            match tokio::fs::metadata(&canonical).await {
                Ok(metadata) => metadata,
                Err(_) => continue,
            }
        } else {
            symlink_metadata
        };

        let is_dir = metadata.is_dir();
        entries.push(DirectoryEntry {
            name,
            size: if is_dir { 0 } else { metadata.len() },
            is_dir,
        });
    }

    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(entries)
}

/// Delete a regular file inside the sandbox. Directories are rejected so the
/// operation can never recursively remove a server volume.
pub async fn delete_file(
    data_root: &str,
    server_id: &str,
    relative: &str,
) -> Result<(), DaemonError> {
    let root = canonical_root(data_root, server_id)?;
    let (path, normalised) = resolve_existing(&root, relative)?;

    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(map_io("inspect file"))?;
    if metadata.is_dir() {
        return Err(DaemonError::BadRequest(
            "directories cannot be deleted".into(),
        ));
    }
    if !metadata.is_file() {
        return Err(DaemonError::BadRequest("path is not a regular file".into()));
    }

    tokio::fs::remove_file(&path)
        .await
        .map_err(map_io("delete file"))?;
    tracing::info!(path = %normalised, "deleted server file");
    Ok(())
}

/// Canonicalise `<data_root>/<server_id>/data`; a missing volume is a 404.
pub(crate) fn canonical_root(data_root: &str, server_id: &str) -> Result<PathBuf, DaemonError> {
    let root = server_data_root(data_root, server_id);
    std::fs::canonicalize(&root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DaemonError::NotFound("server data directory does not exist yet".into())
        } else {
            DaemonError::Internal(format!("resolve server data directory: {error}"))
        }
    })
}

/// Validate a relative path and canonicalise an existing target.
fn resolve_existing(root: &Path, relative: &str) -> Result<(PathBuf, String), DaemonError> {
    let (parts, normalised) = relative_components(relative)?;
    let candidate = parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));

    let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DaemonError::NotFound(format!("file not found: {normalised}"))
        } else {
            DaemonError::Internal(format!("resolve file {normalised}: {error}"))
        }
    })?;

    ensure_within(root, &canonical)?;
    Ok((canonical, normalised))
}

/// Validate a path for a file that may not exist yet. Every existing component
/// (including the final file) is canonicalised and checked, so a symlink cannot
/// redirect the write outside the volume.
fn resolve_for_write(root: &Path, relative: &str) -> Result<(PathBuf, String), DaemonError> {
    let (parts, normalised) = relative_components(relative)?;

    let mut parent = root.to_path_buf();
    for part in &parts[..parts.len() - 1] {
        let next = parent.join(part);
        let canonical = std::fs::canonicalize(&next).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DaemonError::NotFound(format!("parent directory does not exist for {normalised}"))
            } else {
                DaemonError::Internal(format!("resolve path {normalised}: {error}"))
            }
        })?;
        ensure_within(root, &canonical)?;
        parent = canonical;
    }

    let target = parent.join(parts.last().expect("validated non-empty path"));
    if target.exists() {
        let canonical = std::fs::canonicalize(&target).map_err(|error| {
            DaemonError::Internal(format!("resolve file {normalised}: {error}"))
        })?;
        ensure_within(root, &canonical)?;
        return Ok((canonical, normalised));
    }

    Ok((target, normalised))
}

fn ensure_within(root: &Path, candidate: &Path) -> Result<(), DaemonError> {
    if candidate.starts_with(root) {
        Ok(())
    } else {
        tracing::warn!(
            root = %root.display(),
            candidate = %candidate.display(),
            "rejected file access outside the server volume"
        );
        Err(DaemonError::BadRequest(
            "path escapes the server data directory".into(),
        ))
    }
}

/// Reject absolute paths, parent traversal, NUL bytes and empty paths.
/// Returns normal components plus a canonical `a/b/c` string.
pub(crate) fn relative_components(relative: &str) -> Result<(Vec<String>, String), DaemonError> {
    if relative.is_empty() {
        return Err(DaemonError::BadRequest(
            "file path must not be empty".into(),
        ));
    }
    if relative.contains('\0') {
        return Err(DaemonError::BadRequest(
            "file path must not contain NUL bytes".into(),
        ));
    }

    let path = Path::new(relative);
    if path.is_absolute() {
        return Err(DaemonError::BadRequest(
            "file path must be relative to the server data directory".into(),
        ));
    }

    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    DaemonError::BadRequest("file path must be valid UTF-8".into())
                })?;
                if part.is_empty() {
                    return Err(DaemonError::BadRequest(
                        "file path contains an empty part".into(),
                    ));
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(DaemonError::BadRequest(
                    "file path must not contain '..'".into(),
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(DaemonError::BadRequest(
                    "file path must be relative to the server data directory".into(),
                ));
            }
        }
    }

    if parts.is_empty() {
        return Err(DaemonError::BadRequest("file path must name a file".into()));
    }

    Ok((parts.clone(), parts.join("/")))
}

pub(crate) fn map_io(action: &'static str) -> impl Fn(std::io::Error) -> DaemonError {
    move |error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DaemonError::NotFound(format!("{action}: file not found"))
        } else {
            DaemonError::Internal(format!("{action}: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("mc-files-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("create temp root");
        root
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in [
            "../server.properties",
            "config/../../etc/passwd",
            "/etc/passwd",
            "..",
            ".",
            "",
            "a\0b",
        ] {
            assert!(
                relative_components(bad).is_err(),
                "path should be rejected: {bad:?}"
            );
        }
    }

    #[test]
    fn accepts_nested_relative_paths() {
        let (parts, normalised) = relative_components("config/paper-global.yml").expect("valid");
        assert_eq!(parts, vec!["config", "paper-global.yml"]);
        assert_eq!(normalised, "config/paper-global.yml");
    }

    #[tokio::test]
    async fn reads_and_writes_inside_the_volume() {
        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a1";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(&server_dir).expect("create server dir");
        std::fs::write(server_dir.join("server.properties"), "motd=Hello\n").expect("seed file");

        let read = read_text(data_root, server_id, "server.properties")
            .await
            .expect("read");
        assert_eq!(read.content, "motd=Hello\n");
        assert_eq!(read.hash, hash_content("motd=Hello\n"));

        let written = write_text(
            data_root,
            server_id,
            "server.properties",
            &FileWriteRequest {
                content: "motd=Hi\npvp=true\n".into(),
                expected_hash: Some(read.hash),
            },
        )
        .await
        .expect("write");
        assert_eq!(written.content, "motd=Hi\npvp=true\n");

        let on_disk = std::fs::read_to_string(server_dir.join("server.properties")).expect("disk");
        assert_eq!(on_disk, "motd=Hi\npvp=true\n");

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn rejects_stale_hashes() {
        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a2";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(&server_dir).expect("create server dir");
        std::fs::write(server_dir.join("server.properties"), "pvp=false\n").expect("seed");

        let result = write_text(
            data_root,
            server_id,
            "server.properties",
            &FileWriteRequest {
                content: "pvp=true\n".into(),
                expected_hash: Some("stale".into()),
            },
        )
        .await;
        assert!(matches!(result, Err(DaemonError::Conflict(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn lists_directories_and_deletes_files() {
        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a4";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(server_dir.join("config")).expect("create config dir");
        std::fs::write(server_dir.join("server.properties"), "motd=Hello\n").expect("seed file");
        std::fs::write(server_dir.join("config/paper.yml"), "x: 1\n").expect("seed nested");

        let entries = list_directory(data_root, server_id, "")
            .await
            .expect("list root");
        assert_eq!(entries[0].name, "config");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 0);
        assert!(
            entries
                .iter()
                .any(|entry| entry.name == "server.properties")
        );

        let nested = list_directory(data_root, server_id, "config")
            .await
            .expect("list nested");
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].name, "paper.yml");
        assert!(!nested[0].is_dir);
        assert!(nested[0].size > 0);

        let file_as_dir = list_directory(data_root, server_id, "server.properties").await;
        assert!(matches!(file_as_dir, Err(DaemonError::BadRequest(_))));

        delete_file(data_root, server_id, "config/paper.yml")
            .await
            .expect("delete file");
        assert!(!server_dir.join("config/paper.yml").exists());

        let directory = delete_file(data_root, server_id, "config").await;
        assert!(matches!(directory, Err(DaemonError::BadRequest(_))));

        let traversal = delete_file(data_root, server_id, "../outside").await;
        assert!(matches!(traversal, Err(DaemonError::BadRequest(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_escapes() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a3";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(&server_dir).expect("create server dir");

        let outside = root.join("outside.txt");
        std::fs::write(&outside, "secret").expect("seed outside");
        symlink(&outside, server_dir.join("link.txt")).expect("symlink");

        let result = read_text(data_root, server_id, "link.txt").await;
        assert!(
            matches!(result, Err(DaemonError::BadRequest(_))),
            "symlink escaping the volume must be rejected"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn renames_and_copies_within_the_volume() {
        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a5";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(server_dir.join("plugins")).expect("create plugins dir");
        std::fs::write(server_dir.join("server.properties"), "motd=Hello\n").expect("seed file");
        std::fs::write(server_dir.join("plugins/essentials.jar"), "jar").expect("seed plugin");

        rename_path(
            data_root,
            server_id,
            "server.properties",
            "server.properties.bak",
        )
        .await
        .expect("rename file");
        assert!(!server_dir.join("server.properties").exists());
        assert!(server_dir.join("server.properties.bak").exists());

        copy_path(data_root, server_id, "plugins", "plugins-copy")
            .await
            .expect("copy directory");
        assert_eq!(
            std::fs::read_to_string(server_dir.join("plugins-copy/essentials.jar"))
                .expect("copied file"),
            "jar"
        );

        let overwrite = copy_path(
            data_root,
            server_id,
            "plugins/essentials.jar",
            "plugins-copy/essentials.jar",
        )
        .await;
        assert!(matches!(overwrite, Err(DaemonError::Conflict(_))));

        let escape_destination = rename_path(
            data_root,
            server_id,
            "server.properties.bak",
            "../escape.txt",
        )
        .await;
        assert!(matches!(
            escape_destination,
            Err(DaemonError::BadRequest(_))
        ));

        let escape_source = copy_path(data_root, server_id, "../outside", "inside.txt").await;
        assert!(matches!(escape_source, Err(DaemonError::BadRequest(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reads_history_and_updates_access_offline() {
        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a6";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(&server_dir).expect("create server dir");
        std::fs::write(
            server_dir.join("usercache.json"),
            r#"[{"name":"Steve","uuid":"11111111-1111-1111-1111-111111111111","expiresOn":"2027-01-01 00:00:00 +0000"}]"#,
        )
        .expect("seed usercache");

        let history = read_player_history(data_root, server_id)
            .await
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].name, "Steve");
        assert_eq!(history[0].uuid, "11111111-1111-1111-1111-111111111111");
        assert!(history[0].expires_on.is_some());

        let updated = update_player_access_offline(
            data_root,
            server_id,
            "Steve",
            PlayerAccessUpdate {
                operator: Some(true),
                whitelisted: Some(true),
                banned: Some(true),
            },
        )
        .await
        .expect("grant access");
        assert_eq!(updated.operators, vec!["Steve"]);
        assert_eq!(updated.whitelisted, vec!["Steve"]);
        assert_eq!(updated.banned, vec!["Steve"]);

        let ops: Value = serde_json::from_str(
            &std::fs::read_to_string(server_dir.join("ops.json")).expect("read ops file"),
        )
        .expect("read ops");
        assert_eq!(ops[0]["level"], 4);
        let bans: Value = serde_json::from_str(
            &std::fs::read_to_string(server_dir.join("banned-players.json"))
                .expect("read bans file"),
        )
        .expect("read bans");
        assert_eq!(bans[0]["expires"], "forever");

        let revoked = update_player_access_offline(
            data_root,
            server_id,
            "Steve",
            PlayerAccessUpdate {
                operator: Some(false),
                whitelisted: Some(false),
                banned: Some(false),
            },
        )
        .await
        .expect("revoke access");
        assert!(revoked.operators.is_empty());
        assert!(revoked.whitelisted.is_empty());
        assert!(revoked.banned.is_empty());

        let missing = update_player_access_offline(
            data_root,
            server_id,
            "Alex",
            PlayerAccessUpdate {
                operator: Some(true),
                ..PlayerAccessUpdate::default()
            },
        )
        .await;
        assert!(matches!(missing, Err(DaemonError::Conflict(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_download_sources_and_filenames() {
        assert!(
            validate_download_url("https://cdn.modrinth.com/data/a/versions/b/mod.jar").is_ok()
        );
        assert!(validate_download_url("http://cdn.modrinth.com/mod.jar").is_err());
        assert!(validate_download_url("https://localhost/mod.jar").is_err());
        assert!(validate_download_url("https://127.0.0.1/mod.jar").is_err());
        assert!(validate_download_url("https://10.0.0.5/mod.jar").is_err());
        assert!(validate_download_url("https://user:pass@cdn.modrinth.com/mod.jar").is_err());

        assert_eq!(
            validate_download_filename("example-mod-1.0.jar").expect("valid"),
            "example-mod-1.0.jar"
        );
        assert!(validate_download_filename("../escape.jar").is_err());
        assert!(validate_download_filename("nested/file.jar").is_err());
        assert!(validate_download_filename("").is_err());
    }

    #[tokio::test]
    async fn prepares_nested_download_directories_inside_the_volume() {
        let root = temp_root();
        let data_root = root.to_str().expect("utf8 path");
        let server_id = "01a1d8e1-aaaa-7000-8000-0000000006a7";
        let server_dir = server_data_root(data_root, server_id);
        std::fs::create_dir_all(&server_dir).expect("create server dir");
        let canonical_root = std::fs::canonicalize(&server_dir).expect("canonical root");

        let (directory, normalised) = prepare_directory(&canonical_root, "world/datapacks")
            .await
            .expect("prepare nested");
        assert!(directory.starts_with(&canonical_root));
        assert!(directory.is_dir());
        assert_eq!(normalised, "world/datapacks");

        let escape = prepare_directory(&canonical_root, "../outside").await;
        assert!(matches!(escape, Err(DaemonError::BadRequest(_))));

        let _ = std::fs::remove_dir_all(root);
    }
}
