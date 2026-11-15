//! Telemetry payloads shared by the daemon, the control plane and the browser.
//!
//! The daemon emits newline-delimited JSON (`StatsSample` / `LogLine`) on its
//! HMAC-protected streaming endpoints. The control plane re-wraps every item in
//! a [`TelemetryMessage`] envelope and forwards it over the browser WebSocket.

use serde::{Deserialize, Serialize};

/// One point-in-time resource sample for a container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsSample {
    /// CPU usage normalised to 100% == every core assigned to the container.
    pub cpu_percent: f64,
    /// Working-set memory (usage minus reclaimable page cache).
    pub memory_used_bytes: u64,
    /// Container memory limit in bytes (0 when Docker reports no limit).
    pub memory_limit_bytes: u64,
    /// `memory_used_bytes / memory_limit_bytes * 100`, clamped to 0–100.
    pub memory_percent: f64,
    /// Total bytes received across every container interface.
    pub network_rx_bytes: u64,
    /// Total bytes transmitted across every container interface.
    pub network_tx_bytes: u64,
    /// Current process count in the container cgroup.
    pub pids: u64,
    /// Sample time (unix milliseconds).
    pub timestamp_ms: u64,
}

/// Which output stream produced a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl LogStream {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// One log line, already split out of Docker's multiplexed log frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub stream: LogStream,
    /// Docker's timestamp when present, otherwise the daemon's clock.
    pub timestamp_ms: u64,
    /// Message without the Docker timestamp prefix.
    pub message: String,
}

/// Envelope sent over `/api/v1/ws/servers/{id}`.
///
/// Serialised with an internal `type` tag, e.g.
/// `{"type":"stats","sample":{…}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelemetryMessage {
    /// First frame after the socket is accepted.
    Ready {
        server_id: String,
    },
    Stats {
        sample: StatsSample,
    },
    Log {
        line: LogLine,
    },
    /// The daemon-side stream for `stream` (`stats` or `logs`) finished.
    StreamEnded {
        stream: String,
    },
    /// A recoverable forwarding error; the socket stays open.
    Error {
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_messages_use_a_type_tag() {
        let message = TelemetryMessage::Ready {
            server_id: "abc".into(),
        };
        let value = serde_json::to_value(message).expect("serialize ready");
        assert_eq!(value["type"], "ready");
        assert_eq!(value["server_id"], "abc");
    }

    #[test]
    fn log_streams_serialize_lowercase() {
        let line = LogLine {
            stream: LogStream::Stderr,
            timestamp_ms: 42,
            message: "boom".into(),
        };
        let value = serde_json::to_value(&line).expect("serialize line");
        assert_eq!(value["stream"], "stderr");
        assert_eq!(value["message"], "boom");
    }

    #[test]
    fn stats_round_trip() {
        let sample = StatsSample {
            cpu_percent: 12.5,
            memory_used_bytes: 100,
            memory_limit_bytes: 200,
            memory_percent: 50.0,
            network_rx_bytes: 1,
            network_tx_bytes: 2,
            pids: 3,
            timestamp_ms: 4,
        };
        let encoded = serde_json::to_string(&sample).expect("serialize stats");
        let decoded: StatsSample = serde_json::from_str(&encoded).expect("deserialize stats");
        assert_eq!(decoded, sample);
    }
}
