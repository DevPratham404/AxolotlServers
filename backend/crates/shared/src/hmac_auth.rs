//! Shared-secret HMAC request signing for the control plane → daemon channel.
//!
//! Canonical string (exactly four lines):
//!
//! ```text
//! METHOD
//! /path?query
//! unix_timestamp
//! sha256(body) as lowercase hex
//! ```
//!
//! The signature is `hex(HMAC-SHA256(secret, canonical))`, sent in
//! `x-cluster-signature` alongside `x-cluster-timestamp`. Requests older than
//! [`MAX_CLOCK_SKEW_SECS`] are rejected, which bounds replay without a nonce
//! store. TLS protects the secret and the payload in transit; this HMAC proves
//! possession of the shared secret even if the listener is reachable directly.

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// Unix-seconds header.
pub const HMAC_TIMESTAMP_HEADER: &str = "x-cluster-timestamp";
/// Lowercase hex HMAC header.
pub const HMAC_SIGNATURE_HEADER: &str = "x-cluster-signature";
/// Lowercase hex SHA-256 of a streamed request body. Used by the import route
/// so a multi-gigabyte upload never has to be buffered before verification.
pub const HMAC_BODY_SHA256_HEADER: &str = "x-cluster-body-sha256";
/// Maximum accepted clock skew / replay window, in seconds.
pub const MAX_CLOCK_SKEW_SECS: i64 = 300;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HmacError {
    /// Missing/malformed/unparseable timestamp, or outside the skew window.
    TimestampOutOfRange,
    /// Signature header is not valid hex.
    MalformedSignature,
    /// Signature did not match the canonical request.
    InvalidSignature,
}

impl fmt::Display for HmacError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimestampOutOfRange => {
                f.write_str("timestamp missing or outside the allowed window")
            }
            Self::MalformedSignature => f.write_str("signature is not valid hex"),
            Self::InvalidSignature => f.write_str("signature does not match the request"),
        }
    }
}

impl std::error::Error for HmacError {}

/// Build the canonical string that both sides sign.
pub fn canonical_request(
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body: &[u8],
) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path_and_query,
        timestamp,
        hex::encode(Sha256::digest(body))
    )
}

fn mac(secret: &[u8], canonical: &str) -> Result<Vec<u8>, HmacError> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| HmacError::InvalidSignature)?;
    mac.update(canonical.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Canonical string for a streamed body whose SHA-256 was computed separately.
pub fn canonical_request_with_digest(
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body_sha256_hex: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path_and_query,
        timestamp,
        body_sha256_hex.trim().to_ascii_lowercase()
    )
}

/// Compute the lowercase-hex signature for a request whose body digest is
/// already known. The daemon verifies the streamed body against the digest.
pub fn sign_digest(
    secret: &[u8],
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body_sha256_hex: &str,
) -> String {
    let canonical =
        canonical_request_with_digest(method, path_and_query, timestamp, body_sha256_hex);
    mac(secret, &canonical)
        .map(hex::encode)
        .unwrap_or_else(|_| String::new())
}

/// Verify a digest-signed request. The caller must additionally hash the body
/// while streaming it and compare the result to `body_sha256_hex`.
pub fn verify_digest(
    secret: &[u8],
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body_sha256_hex: &str,
    signature_hex: &str,
    now_unix: i64,
) -> Result<(), HmacError> {
    if (now_unix - timestamp).abs() > MAX_CLOCK_SKEW_SECS {
        return Err(HmacError::TimestampOutOfRange);
    }

    let provided = hex::decode(signature_hex.trim()).map_err(|_| HmacError::MalformedSignature)?;
    let canonical =
        canonical_request_with_digest(method, path_and_query, timestamp, body_sha256_hex);

    let mut verifier =
        HmacSha256::new_from_slice(secret).map_err(|_| HmacError::InvalidSignature)?;
    verifier.update(canonical.as_bytes());
    verifier
        .verify_slice(&provided)
        .map_err(|_| HmacError::InvalidSignature)
}

/// Compute the lowercase-hex signature for a request.
pub fn sign(
    secret: &[u8],
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body: &[u8],
) -> String {
    let canonical = canonical_request(method, path_and_query, timestamp, body);
    // Any-length keys are valid for HMAC; a failure here is unreachable.
    mac(secret, &canonical)
        .map(hex::encode)
        .unwrap_or_else(|_| String::new())
}

/// Verify a request signature and its timestamp against `now_unix`.
///
/// Comparison is constant-time via `mac.verify_slice`.
pub fn verify(
    secret: &[u8],
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body: &[u8],
    signature_hex: &str,
    now_unix: i64,
) -> Result<(), HmacError> {
    if (now_unix - timestamp).abs() > MAX_CLOCK_SKEW_SECS {
        return Err(HmacError::TimestampOutOfRange);
    }

    let provided = hex::decode(signature_hex.trim()).map_err(|_| HmacError::MalformedSignature)?;
    let canonical = canonical_request(method, path_and_query, timestamp, body);

    let mut verifier =
        HmacSha256::new_from_slice(secret).map_err(|_| HmacError::InvalidSignature)?;
    verifier.update(canonical.as_bytes());
    verifier
        .verify_slice(&provided)
        .map_err(|_| HmacError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-shared-secret";
    const METHOD: &str = "POST";
    const PATH: &str = "/daemon/servers/abc/start";
    const BODY: &[u8] = br#"{"hello":"world"}"#;
    const NOW: i64 = 1_800_000_000;

    fn signed() -> (i64, String) {
        let timestamp = NOW - 5;
        (timestamp, sign(SECRET, METHOD, PATH, timestamp, BODY))
    }

    #[test]
    fn accepts_valid_signature() {
        let (timestamp, signature) = signed();
        assert_eq!(
            verify(SECRET, METHOD, PATH, timestamp, BODY, &signature, NOW),
            Ok(())
        );
    }

    #[test]
    fn rejects_tampered_body() {
        let (timestamp, signature) = signed();
        assert_eq!(
            verify(SECRET, METHOD, PATH, timestamp, b"{}", &signature, NOW),
            Err(HmacError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_tampered_path_or_method() {
        let (timestamp, signature) = signed();
        assert!(
            verify(
                SECRET,
                METHOD,
                "/daemon/servers/other/start",
                timestamp,
                BODY,
                &signature,
                NOW
            )
            .is_err()
        );
        assert!(verify(SECRET, "DELETE", PATH, timestamp, BODY, &signature, NOW).is_err());
    }

    #[test]
    fn rejects_wrong_secret() {
        let (timestamp, signature) = signed();
        assert_eq!(
            verify(
                b"other-secret",
                METHOD,
                PATH,
                timestamp,
                BODY,
                &signature,
                NOW
            ),
            Err(HmacError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_stale_and_future_timestamps() {
        let signature = sign(SECRET, METHOD, PATH, NOW - MAX_CLOCK_SKEW_SECS - 1, BODY);
        assert_eq!(
            verify(
                SECRET,
                METHOD,
                PATH,
                NOW - MAX_CLOCK_SKEW_SECS - 1,
                BODY,
                &signature,
                NOW
            ),
            Err(HmacError::TimestampOutOfRange)
        );

        let future = NOW + MAX_CLOCK_SKEW_SECS + 1;
        let signature = sign(SECRET, METHOD, PATH, future, BODY);
        assert_eq!(
            verify(SECRET, METHOD, PATH, future, BODY, &signature, NOW),
            Err(HmacError::TimestampOutOfRange)
        );
    }

    #[test]
    fn rejects_malformed_signature() {
        assert_eq!(
            verify(SECRET, METHOD, PATH, NOW, BODY, "not-hex", NOW),
            Err(HmacError::MalformedSignature)
        );
    }

    #[test]
    fn canonical_is_stable() {
        let canonical = canonical_request("post", PATH, 42, b"x");
        assert_eq!(
            canonical,
            format!("POST\n{PATH}\n42\n{}", hex::encode(Sha256::digest(b"x")))
        );
    }

    #[test]
    fn digest_signatures_cover_the_digest_and_request() {
        let digest = hex::encode(Sha256::digest(BODY));
        let timestamp = NOW - 5;
        let signature = sign_digest(SECRET, METHOD, PATH, timestamp, &digest);
        assert_eq!(
            verify_digest(SECRET, METHOD, PATH, timestamp, &digest, &signature, NOW),
            Ok(())
        );
        assert!(
            verify_digest(
                SECRET,
                METHOD,
                PATH,
                timestamp,
                &hex::encode(Sha256::digest(b"other")),
                &signature,
                NOW
            )
            .is_err()
        );
        assert!(verify_digest(SECRET, "PUT", PATH, timestamp, &digest, &signature, NOW).is_err());
    }
}
