//! Minimal Source RCON client.
//!
//! The daemon reaches a container's RCON endpoint through a loopback port
//! published at create time (`127.0.0.1:<derived>`), so this works identically
//! on Linux and Docker Desktop. Credentials are resolved from (in order) the
//! container environment, `server.properties`, or the deterministic per-server
//! password derived from `DAEMON_SHARED_SECRET`.

use std::path::Path;
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::{config::DaemonConfig, error::DaemonError, minecraft};

/// Packet type sent by the client to authenticate.
pub const SERVERDATA_AUTH: i32 = 3;
/// Packet type returned for an authentication attempt (and used for commands).
pub const SERVERDATA_AUTH_RESPONSE: i32 = 2;
/// Packet type sent by the client to execute a command.
pub const SERVERDATA_EXECCOMMAND: i32 = 2;
/// Packet type carrying command output.
pub const SERVERDATA_RESPONSE_VALUE: i32 = 0;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RconPacket {
    pub id: i32,
    pub kind: i32,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RconCredentials {
    pub host: String,
    pub port: u16,
    pub password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RconError {
    #[error("rcon connection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("rcon connection timed out")]
    Timeout,
    #[error("rcon authentication failed")]
    Auth,
    #[error("rcon protocol error: {0}")]
    Protocol(String),
    #[error("rcon is not enabled for this server")]
    Disabled,
}

impl From<RconError> for DaemonError {
    fn from(error: RconError) -> Self {
        match error {
            RconError::Auth => DaemonError::Unauthorized("rcon authentication failed".into()),
            RconError::Disabled => DaemonError::Unavailable(
                "RCON is not enabled for this server; recreate the container to provision RCON"
                    .into(),
            ),
            RconError::Timeout => DaemonError::Unavailable("RCON connection timed out".into()),
            // RCON starts listening several seconds after the container starts.
            // Treat an early refused connection as a retryable state conflict,
            // not a Docker/engine failure or an error-level log.
            RconError::Io(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                DaemonError::Conflict(
                    "server is still starting; RCON is not ready yet — try again in a few seconds"
                        .into(),
                )
            }
            other => DaemonError::Unavailable(other.to_string()),
        }
    }
}

/// Encode one RCON packet: length, request id, type, body, two null bytes.
pub fn encode_packet(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let body = body.as_bytes();
    // length covers id + type + body + two terminating NULs
    let length = 4 + 4 + body.len() + 2;
    let mut packet = Vec::with_capacity(4 + length);
    packet.extend_from_slice(&(length as i32).to_le_bytes());
    packet.extend_from_slice(&id.to_le_bytes());
    packet.extend_from_slice(&kind.to_le_bytes());
    packet.extend_from_slice(body);
    packet.push(0);
    packet.push(0);
    packet
}

/// Decode one packet from `buffer`, returning it plus the number of bytes read.
/// `Ok(None)` means the buffer does not contain a complete packet yet.
pub fn decode_packet(buffer: &[u8]) -> Result<Option<(RconPacket, usize)>, RconError> {
    if buffer.len() < 4 {
        return Ok(None);
    }

    let length = i32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    if length < 10 {
        return Err(RconError::Protocol(format!(
            "invalid packet length {length}"
        )));
    }
    let total = 4 + length as usize;
    if buffer.len() < total {
        return Ok(None);
    }

    let id = i32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
    let kind = i32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
    let body_end = total - 2;
    let body = String::from_utf8_lossy(&buffer[12..body_end]).into_owned();

    Ok(Some((RconPacket { id, kind, body }, total)))
}

/// Execute one command against a running server.
pub async fn execute(
    host: &str,
    port: u16,
    password: &str,
    command: &str,
) -> Result<String, RconError> {
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| RconError::Timeout)??;

    let mut buffer = Vec::with_capacity(4096);

    // Authenticate with request id 1. Servers answer with id -1 on failure.
    write_packet(&mut stream, SERVERDATA_AUTH, 1, password).await?;
    let auth = read_packet(&mut stream, &mut buffer).await?;
    if auth.id == -1 || auth.kind != SERVERDATA_AUTH_RESPONSE {
        return Err(RconError::Auth);
    }

    // Execute with request id 2 and collect the response body.
    write_packet(&mut stream, SERVERDATA_EXECCOMMAND, 2, command).await?;
    let mut body = String::new();
    // Some servers send an empty packet before the real output.
    for _ in 0..4 {
        let packet = read_packet(&mut stream, &mut buffer).await?;
        if packet.kind == SERVERDATA_RESPONSE_VALUE {
            body.push_str(&packet.body);
            if !body.is_empty() {
                break;
            }
        }
    }

    let _ = stream.shutdown().await;
    Ok(body)
}

/// Resolve the endpoint and password for a server's RCON.
pub async fn credentials(
    docker: &Docker,
    config: &DaemonConfig,
    server_id: &str,
) -> Result<RconCredentials, DaemonError> {
    let name = minecraft::container_name(server_id);
    let container = docker
        .inspect_container(&name, None)
        .await
        .map_err(DaemonError::from)?;

    if let Some(password) = container
        .config
        .and_then(|config| config.env)
        .unwrap_or_default()
        .into_iter()
        .find_map(|entry| entry.strip_prefix("RCON_PASSWORD=").map(str::to_string))
        .filter(|password| !password.is_empty())
    {
        return Ok(RconCredentials {
            host: "127.0.0.1".into(),
            port: minecraft::rcon_host_port(server_id),
            password,
        });
    }

    // Containers created before M5 have no RCON env; fall back to the file that
    // itzg writes on first start.
    let properties_path = Path::new(&config.data_root)
        .join(server_id)
        .join("data")
        .join("server.properties");
    if let Ok(contents) = tokio::fs::read_to_string(&properties_path).await {
        let enabled = property(&contents, "enable-rcon")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let password = property(&contents, "rcon.password").filter(|value| !value.is_empty());

        if enabled {
            if let Some(password) = password {
                return Ok(RconCredentials {
                    host: "127.0.0.1".into(),
                    port: minecraft::rcon_host_port(server_id),
                    password,
                });
            }
        }
    }

    Err(RconError::Disabled.into())
}

async fn write_packet(
    stream: &mut TcpStream,
    kind: i32,
    id: i32,
    body: &str,
) -> Result<(), RconError> {
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&encode_packet(id, kind, body)))
        .await
        .map_err(|_| RconError::Timeout)??;
    tokio::time::timeout(IO_TIMEOUT, stream.flush())
        .await
        .map_err(|_| RconError::Timeout)??;
    Ok(())
}

async fn read_packet(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
) -> Result<RconPacket, RconError> {
    loop {
        if let Some((packet, consumed)) = decode_packet(buffer)? {
            buffer.drain(..consumed);
            return Ok(packet);
        }

        let mut chunk = [0u8; 4096];
        let read = tokio::time::timeout(IO_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| RconError::Timeout)??;

        if read == 0 {
            return Err(RconError::Protocol("connection closed".into()));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

/// Read a single `key=value` from a properties file.
pub fn property(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_source_packet_layout() {
        let packet = encode_packet(7, SERVERDATA_AUTH, "hunter2");
        // 4 length + 4 id + 4 type + 7 body + 2 nulls
        assert_eq!(packet.len(), 4 + 4 + 4 + 7 + 2);
        assert_eq!(
            i32::from_le_bytes(packet[0..4].try_into().expect("length bytes")),
            17
        );
        assert_eq!(
            i32::from_le_bytes(packet[4..8].try_into().expect("id bytes")),
            7
        );
        assert_eq!(
            i32::from_le_bytes(packet[8..12].try_into().expect("type bytes")),
            SERVERDATA_AUTH
        );
        assert_eq!(&packet[12..19], b"hunter2");
        assert_eq!(&packet[19..], &[0, 0]);
    }

    #[test]
    fn decodes_incremental_packets() {
        let first = encode_packet(1, SERVERDATA_RESPONSE_VALUE, "hello");
        let second = encode_packet(2, SERVERDATA_RESPONSE_VALUE, "world");

        // Incomplete buffer yields no packet.
        assert!(decode_packet(&first[..6]).expect("partial").is_none());

        let mut combined = first;
        combined.extend_from_slice(&second);

        let (packet, consumed) = decode_packet(&combined)
            .expect("decode")
            .expect("complete packet");
        assert_eq!(
            packet,
            RconPacket {
                id: 1,
                kind: SERVERDATA_RESPONSE_VALUE,
                body: "hello".into(),
            }
        );
        assert_eq!(consumed, 4 + 4 + 4 + 5 + 2);

        let (packet, consumed) = decode_packet(&combined[consumed..])
            .expect("decode")
            .expect("second packet");
        assert_eq!(packet.body, "world");
        assert_eq!(consumed, combined.len() - (4 + 4 + 4 + 5 + 2));
    }

    #[test]
    fn rejects_impossible_lengths() {
        let mut buffer = encode_packet(1, 0, "");
        buffer[0] = 1;
        assert!(decode_packet(&buffer).is_err());
    }

    #[test]
    fn parses_properties() {
        let contents = "\
#Minecraft server properties
enable-rcon=true
rcon.password=super-secret
motd=A = sign in value
";
        assert_eq!(property(contents, "enable-rcon").as_deref(), Some("true"));
        assert_eq!(
            property(contents, "rcon.password").as_deref(),
            Some("super-secret")
        );
        assert_eq!(
            property(contents, "motd").as_deref(),
            Some("A = sign in value")
        );
        assert_eq!(property(contents, "missing"), None);
    }

    #[test]
    fn credentials_target_the_loopback_mapping() {
        // Pure sanity check of the endpoint convention used by `execute`.
        let port = minecraft::rcon_host_port("01a1d37d-4700-7107-a4e8-72e044c3a464");
        assert!((30_000..=39_999).contains(&port));
        assert_eq!(minecraft::RCON_PORT, 25_575);
    }

    /// Exercises auth + exec framing against a loopback RCON server.
    #[tokio::test]
    async fn authenticates_and_executes_over_tcp() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake rcon");
        let port = listener.local_addr().expect("addr").port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buffer = Vec::new();

            let auth = read_packet(&mut socket, &mut buffer)
                .await
                .expect("auth packet");
            assert_eq!(auth.kind, SERVERDATA_AUTH);
            assert_eq!(auth.body, "hunter2");
            socket
                .write_all(&encode_packet(auth.id, SERVERDATA_AUTH_RESPONSE, ""))
                .await
                .expect("auth response");

            let exec = read_packet(&mut socket, &mut buffer)
                .await
                .expect("exec packet");
            assert_eq!(exec.kind, SERVERDATA_EXECCOMMAND);
            assert_eq!(exec.body, "list");
            socket
                .write_all(&encode_packet(
                    exec.id,
                    SERVERDATA_RESPONSE_VALUE,
                    "There are 0 of a max of 20 players online",
                ))
                .await
                .expect("command response");
        });

        let output = execute("127.0.0.1", port, "hunter2", "list")
            .await
            .expect("execute command");
        assert_eq!(output, "There are 0 of a max of 20 players online");
        server.await.expect("fake server");
    }

    #[tokio::test]
    async fn rejects_bad_credentials() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake rcon");
        let port = listener.local_addr().expect("addr").port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buffer = Vec::new();
            let auth = read_packet(&mut socket, &mut buffer)
                .await
                .expect("auth packet");
            socket
                .write_all(&encode_packet(-1, SERVERDATA_AUTH_RESPONSE, ""))
                .await
                .expect("auth failure response");
            assert_eq!(auth.kind, SERVERDATA_AUTH);
        });

        let result = execute("127.0.0.1", port, "wrong", "list").await;
        assert!(matches!(result, Err(RconError::Auth)));
        server.await.expect("fake server");
    }
}
