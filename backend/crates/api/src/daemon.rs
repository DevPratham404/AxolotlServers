//! Typed, HMAC-signed client for the node daemon control API.
//!
//! Every request is signed with the shared secret using the canonical form in
//! `cluster_shared::hmac_auth`, so the daemon can verify authenticity and
//! integrity even before TLS termination is added in front of it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use cluster_shared::{
    AddonDownloadRequest, AddonDownloadResult, ApiErrorBody, CreateServerSpec, DirectoryEntry,
    FileContent, FileCopyRequest, FileRenameRequest, FileWriteRequest, HMAC_BODY_SHA256_HEADER,
    HMAC_SIGNATURE_HEADER, HMAC_TIMESTAMP_HEADER, ImportServerResult, ModpackInstallRequest,
    ModpackInstallResult, PlayerAccessLists, PlayerAccessUpdate, PlayerHistoryEntry,
    RconCommandRequest, RconCommandResponse, ServerResources, sign, sign_digest,
};
use futures_util::stream;
use reqwest::{Body, Method, StatusCode, header};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::error::ApiError;

/// Where imported server data comes from. ZIP files are streamed from a
/// temporary file; local paths are read by the daemon on its own host.
pub enum DaemonImportSource {
    Zip { path: PathBuf },
    LocalPath(String),
}

#[derive(Clone)]
pub struct DaemonClient {
    base_url: String,
    secret: Arc<Vec<u8>>,
    http: reqwest::Client,
}

impl DaemonClient {
    pub fn new(
        base_url: impl Into<String>,
        shared_secret: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| anyhow::anyhow!("build daemon HTTP client: {error}"))?;

        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            secret: Arc::new(shared_secret.into().into_bytes()),
            http,
        })
    }

    pub async fn create_server(&self, spec: &CreateServerSpec) -> Result<(), ApiError> {
        let body = serde_json::to_vec(spec)
            .map_err(|error| ApiError::Internal(format!("serialize server spec: {error}")))?;
        let path = format!("/daemon/servers/{}/create", spec.id);
        self.send(Method::POST, &path, &body).await
    }

    /// Stream existing server data to the daemon's import endpoint. Uploaded
    /// ZIPs are sent as multipart from a temporary file; long transfers use a
    /// six-hour request timeout and digest-based HMAC so the body is never
    /// buffered in memory.
    pub async fn import_server(
        &self,
        server_id: &str,
        source: DaemonImportSource,
    ) -> Result<ImportServerResult, ApiError> {
        let path = "/daemon/servers/import";
        let boundary = format!("cluster-{}", uuid::Uuid::now_v7());

        let (prefix, file_path, suffix) = match source {
            DaemonImportSource::LocalPath(local_path) => {
                if local_path.chars().any(char::is_control) {
                    return Err(ApiError::BadRequest(
                        "local_path must not contain control characters".into(),
                    ));
                }
                let body = format!(
                    "--{boundary}\r\n\
                     Content-Disposition: form-data; name=\"server_id\"\r\n\r\n\
                     {server_id}\r\n\
                     --{boundary}\r\n\
                     Content-Disposition: form-data; name=\"local_path\"\r\n\r\n\
                     {local_path}\r\n\
                     --{boundary}--\r\n"
                );
                (body.into_bytes(), None, Vec::new())
            }
            DaemonImportSource::Zip { path: upload } => {
                let prefix = format!(
                    "--{boundary}\r\n\
                     Content-Disposition: form-data; name=\"server_id\"\r\n\r\n\
                     {server_id}\r\n\
                     --{boundary}\r\n\
                     Content-Disposition: form-data; name=\"file\"; filename=\"import.zip\"\r\n\
                     Content-Type: application/zip\r\n\r\n"
                );
                let suffix = format!("\r\n--{boundary}--\r\n");
                (prefix.into_bytes(), Some(upload), suffix.into_bytes())
            }
        };

        let digest = match &file_path {
            Some(path) => hash_file_multipart(&prefix, path, &suffix).await?,
            None => hex::encode(Sha256::digest(
                [prefix.as_slice(), suffix.as_slice()].concat(),
            )),
        };
        let timestamp = now_unix();
        let signature = sign_digest(
            &self.secret,
            Method::POST.as_str(),
            path,
            timestamp,
            &digest,
        );

        let body_stream = multipart_stream(prefix, file_path, suffix)?;
        let request = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .header(HMAC_TIMESTAMP_HEADER, timestamp.to_string())
            .header(HMAC_SIGNATURE_HEADER, signature)
            .header(HMAC_BODY_SHA256_HEADER, digest)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .timeout(Duration::from_secs(6 * 60 * 60))
            .body(Body::wrap_stream(body_stream));

        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Unavailable(format!("daemon unreachable: {error}")))?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ApiError::Unavailable(format!("read daemon response: {error}")))?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            return Err(map_daemon_error(status, &text));
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| ApiError::Internal(format!("decode daemon response: {error}")))
    }

    pub async fn start_server(
        &self,
        server_id: &str,
        resources: ServerResources,
    ) -> Result<(), ApiError> {
        let body = serde_json::to_vec(&resources)
            .map_err(|error| ApiError::Internal(format!("serialize resources: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/start");
        self.send(Method::POST, &path, &body).await
    }

    pub async fn stop_server(&self, server_id: &str) -> Result<(), ApiError> {
        let path = format!("/daemon/servers/{server_id}/stop");
        self.send(Method::POST, &path, b"").await
    }

    pub async fn restart_server(
        &self,
        server_id: &str,
        resources: ServerResources,
    ) -> Result<(), ApiError> {
        let body = serde_json::to_vec(&resources)
            .map_err(|error| ApiError::Internal(format!("serialize resources: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/restart");
        self.send(Method::POST, &path, &body).await
    }

    pub async fn delete_server(&self, server_id: &str) -> Result<(), ApiError> {
        let path = format!("/daemon/servers/{server_id}");
        self.send(Method::DELETE, &path, b"").await
    }

    /// Execute a raw command over the daemon's RCON bridge and return the
    /// server's textual response.
    pub async fn run_command(&self, server_id: &str, command: &str) -> Result<String, ApiError> {
        let request = RconCommandRequest {
            command: command.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::Internal(format!("serialize rcon command: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/command");
        let response: RconCommandResponse = self.send_json(Method::POST, &path, &body).await?;
        Ok(response.output)
    }

    /// Read a text file from a server's data volume.
    pub async fn read_file(
        &self,
        server_id: &str,
        relative_path: &str,
    ) -> Result<FileContent, ApiError> {
        let path = file_path(server_id, relative_path);
        self.send_json(Method::GET, &path, b"").await
    }

    /// Atomically replace a text file in a server's data volume.
    pub async fn write_file(
        &self,
        server_id: &str,
        relative_path: &str,
        content: &str,
        expected_hash: Option<&str>,
    ) -> Result<FileContent, ApiError> {
        let request = FileWriteRequest {
            content: content.to_string(),
            expected_hash: expected_hash.map(ToString::to_string),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::Internal(format!("serialize file write: {error}")))?;
        let path = file_path(server_id, relative_path);
        self.send_json(Method::PUT, &path, &body).await
    }

    /// List the immediate children of a directory in a server's data volume.
    /// An empty `relative_path` addresses the volume root.
    pub async fn list_directory(
        &self,
        server_id: &str,
        relative_path: &str,
    ) -> Result<Vec<DirectoryEntry>, ApiError> {
        let path = directory_path(server_id, relative_path);
        self.send_json(Method::GET, &path, b"").await
    }

    /// Delete a regular file from a server's data volume.
    pub async fn delete_file(&self, server_id: &str, relative_path: &str) -> Result<(), ApiError> {
        let path = file_path(server_id, relative_path);
        self.send(Method::DELETE, &path, b"").await
    }

    /// Fetch the list of online players via the daemon's RCON bridge.
    pub async fn get_players(&self, server_id: &str) -> Result<Vec<String>, ApiError> {
        let path = format!("/daemon/servers/{server_id}/players");
        self.send_json(Method::GET, &path, b"").await
    }

    /// Fetch players recorded in `usercache.json` (works while stopped).
    pub async fn get_player_history(
        &self,
        server_id: &str,
    ) -> Result<Vec<PlayerHistoryEntry>, ApiError> {
        let path = format!("/daemon/servers/{server_id}/players/history");
        self.send_json(Method::GET, &path, b"").await
    }

    pub async fn get_player_access(&self, server_id: &str) -> Result<PlayerAccessLists, ApiError> {
        let path = format!("/daemon/servers/{server_id}/players/access");
        self.send_json(Method::GET, &path, b"").await
    }

    pub async fn update_player_access(
        &self,
        server_id: &str,
        username: &str,
        update: PlayerAccessUpdate,
    ) -> Result<PlayerAccessLists, ApiError> {
        let body = serde_json::to_vec(&update)
            .map_err(|error| ApiError::Internal(format!("serialize player access: {error}")))?;
        let username = percent_encode_segment(username);
        let path = format!("/daemon/servers/{server_id}/players/{username}/access");
        self.send_json(Method::PUT, &path, &body).await
    }

    /// Rename or move a path inside the server volume.
    pub async fn rename_file(
        &self,
        server_id: &str,
        old_path: &str,
        new_path: &str,
    ) -> Result<(), ApiError> {
        let request = FileRenameRequest {
            old_path: old_path.to_string(),
            new_path: new_path.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::Internal(format!("serialize rename: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/files/rename");
        self.send(Method::POST, &path, &body).await
    }

    /// Copy a path inside the server volume.
    pub async fn copy_file(
        &self,
        server_id: &str,
        source_path: &str,
        dest_path: &str,
    ) -> Result<(), ApiError> {
        let request = FileCopyRequest {
            source_path: source_path.to_string(),
            dest_path: dest_path.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::Internal(format!("serialize copy: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/files/copy");
        self.send(Method::POST, &path, &body).await
    }

    /// Stream a remote add-on file into a directory inside the server volume.
    pub async fn download_addon(
        &self,
        server_id: &str,
        url: &str,
        filename: &str,
        target_directory: &str,
    ) -> Result<AddonDownloadResult, ApiError> {
        let request = AddonDownloadRequest {
            url: url.to_string(),
            filename: filename.to_string(),
            target_directory: target_directory.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::Internal(format!("serialize download: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/downloads");
        self.send_json(Method::POST, &path, &body).await
    }

    /// Install a Modrinth `.mrpack` through the daemon's modpack engine. Pack
    /// extraction can take minutes, so this call gets a much longer timeout than
    /// ordinary control-plane requests.
    pub async fn install_modpack(
        &self,
        server_id: &str,
        url: &str,
    ) -> Result<ModpackInstallResult, ApiError> {
        let request = ModpackInstallRequest {
            url: url.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::Internal(format!("serialize modpack install: {error}")))?;
        let path = format!("/daemon/servers/{server_id}/modpacks/install");
        self.send_json_timeout(Method::POST, &path, &body, Duration::from_secs(900))
            .await
    }

    /// Download raw bytes of a file from the server's data volume.
    pub async fn read_raw_file(
        &self,
        server_id: &str,
        relative_path: &str,
    ) -> Result<Vec<u8>, ApiError> {
        let path = raw_file_path(server_id, relative_path);
        self.send_bytes(Method::GET, &path, b"").await
    }

    /// Upload raw bytes to a file in the server's data volume (binary-safe).
    pub async fn upload_file(
        &self,
        server_id: &str,
        relative_path: &str,
        data: &[u8],
    ) -> Result<(), ApiError> {
        let path = raw_file_path(server_id, relative_path);
        // We need a custom send that sets application/octet-stream.
        self.send_raw_upload(Method::PUT, &path, data).await
    }

    /// Create a directory inside the server's data volume.
    pub async fn create_directory(
        &self,
        server_id: &str,
        relative_path: &str,
    ) -> Result<(), ApiError> {
        let path = directory_path(server_id, relative_path);
        self.send(Method::POST, &path, b"").await
    }

    /// PUT raw bytes to the daemon with `application/octet-stream` content-type.
    async fn send_raw_upload(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
    ) -> Result<(), ApiError> {
        let timestamp = now_unix();
        let signature = sign(&self.secret, method.as_str(), path, timestamp, body);

        let request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .header(HMAC_TIMESTAMP_HEADER, timestamp.to_string())
            .header(HMAC_SIGNATURE_HEADER, signature)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body.to_vec());

        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Unavailable(format!("daemon unreachable: {error}")))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_daemon_error(status, &text));
        }
        Ok(())
    }

    /// Open a signed streaming GET request (stats/logs) and hand the response
    /// body back as a byte stream. The caller splits it into NDJSON lines.
    pub async fn open_stream(
        &self,
        path: &str,
    ) -> Result<impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>>, ApiError>
    {
        let timestamp = now_unix();
        let signature = sign(&self.secret, Method::GET.as_str(), path, timestamp, b"");

        let response = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header(HMAC_TIMESTAMP_HEADER, timestamp.to_string())
            .header(HMAC_SIGNATURE_HEADER, signature)
            .send()
            .await
            .map_err(|error| ApiError::Unavailable(format!("daemon unreachable: {error}")))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_daemon_error(status, &text));
        }

        Ok(response.bytes_stream())
    }

    async fn send(&self, method: Method, path: &str, body: &[u8]) -> Result<(), ApiError> {
        self.send_bytes(method, path, body).await.map(|_| ())
    }

    /// Like [`Self::send`], but decodes a JSON response body.
    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
    ) -> Result<T, ApiError> {
        let bytes = self.send_bytes(method, path, body).await?;
        serde_json::from_slice(&bytes)
            .map_err(|error| ApiError::Internal(format!("decode daemon response: {error}")))
    }

    /// Like [`Self::send_json`], but overrides the client's per-request timeout.
    async fn send_json_timeout<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
        timeout: Duration,
    ) -> Result<T, ApiError> {
        let bytes = self
            .send_bytes_with_timeout(method, path, body, Some(timeout))
            .await?;
        serde_json::from_slice(&bytes)
            .map_err(|error| ApiError::Internal(format!("decode daemon response: {error}")))
    }

    async fn send_bytes(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
    ) -> Result<Vec<u8>, ApiError> {
        self.send_bytes_with_timeout(method, path, body, None).await
    }

    async fn send_bytes_with_timeout(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
        timeout: Option<Duration>,
    ) -> Result<Vec<u8>, ApiError> {
        let timestamp = now_unix();
        let signature = sign(&self.secret, method.as_str(), path, timestamp, body);

        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .header(HMAC_TIMESTAMP_HEADER, timestamp.to_string())
            .header(HMAC_SIGNATURE_HEADER, signature);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }

        if !body.is_empty() {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }

        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Unavailable(format!("daemon unreachable: {error}")))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_daemon_error(status, &text));
        }

        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| ApiError::Unavailable(format!("read daemon response: {error}")))
    }
}

fn map_daemon_error(status: StatusCode, text: &str) -> ApiError {
    let message = parse_daemon_message(text);
    match status {
        StatusCode::BAD_REQUEST => ApiError::BadRequest(message),
        StatusCode::NOT_FOUND => ApiError::NotFound(message),
        StatusCode::CONFLICT => ApiError::Conflict(message),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ApiError::Internal(
            "daemon rejected the control-plane signature (check DAEMON_SHARED_SECRET)".into(),
        ),
        StatusCode::SERVICE_UNAVAILABLE => ApiError::DependencyUnavailable(message),
        _ => ApiError::Unavailable(message),
    }
}

fn parse_daemon_message(text: &str) -> String {
    serde_json::from_str::<ApiErrorBody>(text)
        .map(|body| body.message)
        .unwrap_or_else(|_| "daemon request failed".to_string())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

struct MultipartState {
    prefix: Option<Bytes>,
    file: Option<tokio::fs::File>,
    suffix: Option<Bytes>,
    buffer: Vec<u8>,
}

async fn hash_file_multipart(
    prefix: &[u8],
    path: &std::path::Path,
    suffix: &[u8],
) -> Result<String, ApiError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| ApiError::BadRequest(format!("open import archive: {error}")))?;
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| ApiError::BadRequest(format!("read import archive: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    hasher.update(suffix);
    Ok(hex::encode(hasher.finalize()))
}

fn multipart_stream(
    prefix: Vec<u8>,
    file_path: Option<PathBuf>,
    suffix: Vec<u8>,
) -> Result<impl futures_util::Stream<Item = Result<Bytes, std::io::Error>>, ApiError> {
    let file = match file_path {
        Some(path) => Some(tokio::fs::File::from_std(
            std::fs::File::open(&path)
                .map_err(|error| ApiError::BadRequest(format!("open import archive: {error}")))?,
        )),
        None => None,
    };
    let state = MultipartState {
        prefix: Some(Bytes::from(prefix)),
        file,
        suffix: Some(Bytes::from(suffix)),
        buffer: vec![0u8; 64 * 1024],
    };

    Ok(stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(prefix) = state.prefix.take() {
                return Ok::<_, std::io::Error>(Some((prefix, state)));
            }
            if let Some(mut file) = state.file.take() {
                let read = match file.read(&mut state.buffer).await {
                    Ok(read) => read,
                    Err(error) => return Err(error),
                };
                if read == 0 {
                    continue;
                }
                let chunk = Bytes::copy_from_slice(&state.buffer[..read]);
                state.file = Some(file);
                return Ok(Some((chunk, state)));
            }
            if let Some(suffix) = state.suffix.take() {
                return Ok(Some((suffix, state)));
            }
            return Ok(None);
        }
    }))
}

/// Build the daemon file path with each segment percent-encoded. The encoded
/// value is what is signed and what the daemon verifies.
fn file_path(server_id: &str, relative_path: &str) -> String {
    let encoded = encode_relative_path(relative_path);
    format!("/daemon/servers/{server_id}/files/{encoded}")
}

fn raw_file_path(server_id: &str, relative_path: &str) -> String {
    let encoded = encode_relative_path(relative_path);
    format!("/daemon/servers/{server_id}/raw-files/{encoded}")
}

fn directory_path(server_id: &str, relative_path: &str) -> String {
    if relative_path.is_empty() {
        return format!("/daemon/servers/{server_id}/directory");
    }
    let encoded = encode_relative_path(relative_path);
    format!("/daemon/servers/{server_id}/directory/{encoded}")
}

fn encode_relative_path(relative_path: &str) -> String {
    relative_path
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_file_paths_for_signing() {
        assert_eq!(
            file_path("abc", "server.properties"),
            "/daemon/servers/abc/files/server.properties"
        );
        assert_eq!(
            file_path("abc", "config/paper global.yml"),
            "/daemon/servers/abc/files/config/paper%20global.yml"
        );
        assert_eq!(
            file_path("abc", "config/../../../etc/passwd"),
            "/daemon/servers/abc/files/config/../../../etc/passwd"
        );
        assert_eq!(directory_path("abc", ""), "/daemon/servers/abc/directory");
        assert_eq!(
            directory_path("abc", "mods/configs"),
            "/daemon/servers/abc/directory/mods/configs"
        );
    }
}
