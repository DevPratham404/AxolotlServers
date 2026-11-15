//! Splitting Docker's multiplexed log frames into individual, timestamped lines
//! for the graphical console.

use bollard::container::LogOutput;
use bytes::Bytes;
use cluster_shared::{LogLine, LogStream};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::stats::now_ms;

/// Convert one Docker log frame into zero or more lines.
///
/// Docker frames can contain several newline-separated messages, and when
/// `timestamps(true)` is used every line is prefixed with an RFC 3339 stamp.
pub fn lines_from_output(output: LogOutput) -> Vec<LogLine> {
    let stream = match &output {
        LogOutput::StdErr { .. } => LogStream::Stderr,
        LogOutput::StdOut { .. } | LogOutput::Console { .. } | LogOutput::StdIn { .. } => {
            LogStream::Stdout
        }
    };

    let bytes: Bytes = output.into_bytes();
    let text = String::from_utf8_lossy(&bytes);

    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (timestamp_ms, message) = split_timestamp(line);
            LogLine {
                stream,
                timestamp_ms,
                message: strip_ansi(message),
            }
        })
        .collect()
}

/// Split `2026-11-13T12:00:00.123456789Z [Server thread/INFO]: hi` into the
/// parsed timestamp and the remaining message.
fn split_timestamp(line: &str) -> (u64, &str) {
    let Some((candidate, rest)) = line.split_once(' ') else {
        return (now_ms(), line);
    };

    match OffsetDateTime::parse(candidate, &Rfc3339) {
        Ok(timestamp) => {
            let millis = timestamp.unix_timestamp_nanos() / 1_000_000;
            (u64::try_from(millis).unwrap_or(0), rest)
        }
        Err(_) => (now_ms(), line),
    }
}

/// Remove ANSI SGR escape sequences (`ESC [ … m`) that some JVMs emit even when
/// output is not a TTY.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.next() == Some('[') {
                // Consume parameters until the final command byte.
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            continue;
        }
        out.push(ch);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdout(text: &str) -> LogOutput {
        LogOutput::StdOut {
            message: Bytes::from(text.to_string()),
        }
    }

    #[test]
    fn splits_multiple_lines_and_keeps_streams() {
        let lines = lines_from_output(stdout("first\nsecond\n"));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].message, "first");
        assert_eq!(lines[1].message, "second");
        assert!(lines.iter().all(|line| line.stream == LogStream::Stdout));
    }

    #[test]
    fn strips_docker_timestamp_prefix() {
        let lines = lines_from_output(stdout(
            "2026-11-13T12:00:00.123456789Z [Server thread/INFO]: Done (1.2s)!\n",
        ));
        assert_eq!(lines[0].message, "[Server thread/INFO]: Done (1.2s)!");
        assert!(lines[0].timestamp_ms > 1_700_000_000_000);
    }

    #[test]
    fn stderr_maps_to_stderr() {
        let lines = lines_from_output(LogOutput::StdErr {
            message: Bytes::from_static(b"boom\n"),
        });
        assert_eq!(lines[0].stream, LogStream::Stderr);
    }

    #[test]
    fn strips_ansi_colours() {
        let lines = lines_from_output(stdout("\u{1b}[32mINFO\u{1b}[0m ready"));
        assert_eq!(lines[0].message, "INFO ready");
    }

    #[test]
    fn ignores_blank_lines() {
        assert!(lines_from_output(stdout("\n\r\n")).is_empty());
    }
}
