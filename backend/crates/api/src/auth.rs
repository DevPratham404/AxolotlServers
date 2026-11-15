use std::sync::Arc;

use argon2::{Argon2, PasswordHasher, PasswordVerifier, password_hash::phc::PasswordHash};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::ApiError;

/// Claims carried by every issued access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// User id (UUIDv7).
    pub sub: String,
    pub username: String,
    /// True until the user rotates the bootstrap/initial password.
    #[serde(default)]
    pub requires_password_change: bool,
    /// Issued at (unix seconds).
    pub iat: i64,
    /// Expiry (unix seconds).
    pub exp: i64,
}

/// Authenticated principal injected into request extensions by `require_auth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub requires_password_change: bool,
}

/// Issues and validates HS256 JWTs. Cheap to clone; holds no per-request state.
#[derive(Clone)]
pub struct AuthService {
    secret: Arc<String>,
    ttl_secs: i64,
}

impl AuthService {
    pub fn new(secret: impl Into<String>, ttl_hours: i64) -> Self {
        Self {
            secret: Arc::new(secret.into()),
            ttl_secs: ttl_hours.max(1) * 3600,
        }
    }

    /// Returns the signed token and its expiry (unix seconds).
    pub fn issue(&self, user: &AuthUser) -> Result<(String, i64), ApiError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let exp = now + self.ttl_secs;
        let claims = Claims {
            sub: user.id.clone(),
            username: user.username.clone(),
            requires_password_change: user.requires_password_change,
            iat: now,
            exp,
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|error| ApiError::Internal(format!("jwt encode: {error}")))?;

        Ok((token, exp))
    }

    /// Validates signature, algorithm and expiry, returning the principal.
    pub fn verify(&self, token: &str) -> Result<AuthUser, ApiError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub"]);

        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|_| ApiError::Unauthorized("invalid or expired token".into()))?;

        Ok(AuthUser {
            id: data.claims.sub,
            username: data.claims.username,
            requires_password_change: data.claims.requires_password_change,
        })
    }
}

/// Hash a plaintext password with argon2id (default parameters).
///
/// CPU-bound: call through `tokio::task::spawn_blocking` from async code.
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::Internal(format!("password hash: {error}")))
}

/// Constant-time verification of a plaintext password against a stored hash.
///
/// CPU-bound: call through `tokio::task::spawn_blocking` from async code.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password(
            "correct horse battery staple",
            "not-a-hash"
        ));
    }

    #[test]
    fn jwt_round_trip() {
        let service = AuthService::new("test-secret", 1);
        let user = AuthUser {
            id: "user-1".into(),
            username: "admin".into(),
            requires_password_change: false,
        };
        let (token, exp) = service.issue(&user).expect("issue");
        assert!(exp > OffsetDateTime::now_utc().unix_timestamp());

        let verified = service.verify(&token).expect("verify");
        assert_eq!(verified.id, user.id);
        assert_eq!(verified.username, user.username);
    }

    #[test]
    fn jwt_rejects_other_secret() {
        let issuer = AuthService::new("secret-a", 1);
        let verifier = AuthService::new("secret-b", 1);
        let (token, _) = issuer
            .issue(&AuthUser {
                id: "u".into(),
                username: "u".into(),
                requires_password_change: false,
            })
            .expect("issue");
        assert!(verifier.verify(&token).is_err());
    }

    #[test]
    fn jwt_rejects_garbage() {
        let service = AuthService::new("secret", 1);
        assert!(service.verify("not-a-token").is_err());
    }
}
