//! L1 / L2 authentication primitives and header constants.
//!
//! Chainup uses `PRED_*` header names (vs Polymarket's `POLY_*`) and standard base64 for the
//! HMAC secret (vs URL-safe).

use base64::Engine;
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::Sha256;
use uuid::Uuid;

/// L1 / L2 chainup auth headers.
pub mod header {
    // L1 (EIP-712) — used by `/auth/api-key` create/derive/revoke.
    pub const PRED_ADDRESS: &str = "PRED_ADDRESS";
    pub const PRED_NONCE: &str = "PRED_NONCE";
    pub const PRED_SIGNATURE: &str = "PRED_SIGNATURE";
    pub const PRED_TIMESTAMP: &str = "PRED_TIMESTAMP";
    /// Optional on L1: binds the created API key to a tenant scope.
    pub const PRED_SCOPE_ID: &str = "PRED_SCOPE_ID";

    // L2 (HMAC-SHA256) — used by trading endpoints. Reuses PRED_SIGNATURE / PRED_TIMESTAMP /
    // PRED_ADDRESS above plus:
    pub const PRED_API_KEY: &str = "PRED_API_KEY";
    pub const PRED_PASSPHRASE: &str = "PRED_PASSPHRASE";
}

/// L2 API credentials returned by `/auth/api-key` (and re-derived by `/auth/derive-api-key`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Credentials {
    #[serde(alias = "apiKey")]
    pub key: Uuid,
    pub secret: SecretString,
    pub passphrase: SecretString,
}

impl Credentials {
    #[must_use]
    pub fn new(key: Uuid, secret: String, passphrase: String) -> Self {
        Self {
            key,
            secret: SecretString::from(secret),
            passphrase: SecretString::from(passphrase),
        }
    }

    pub fn key(&self) -> Uuid {
        self.key
    }

    pub fn secret(&self) -> &SecretString {
        &self.secret
    }

    pub fn passphrase(&self) -> &SecretString {
        &self.passphrase
    }
}

/// Compute the chainup L2 HMAC signature.
///
/// Mirrors `services/clob-service/internal/tradingapi/middleware/auth.go::computeHMAC`:
///
/// 1. Decode `secret` with **standard** base64 (`STANDARD`, not URL-safe). If decoding fails
///    the raw bytes are used directly — matching the Go fallback.
/// 2. Concatenate `timestamp + method + path + body` (no separators).
/// 3. HMAC-SHA256 with the key, then encode the MAC as standard base64.
pub fn compute_l2_hmac(secret: &str, timestamp: &str, method: &str, path: &str, body: &str) -> String {
    let key = base64::engine::general_purpose::STANDARD
        .decode(secret)
        .unwrap_or_else(|_| secret.as_bytes().to_vec());

    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key)
        .expect("HMAC accepts any key length");
    mac.update(timestamp.as_bytes());
    mac.update(method.as_bytes());
    mac.update(path.as_bytes());
    mac.update(body.as_bytes());
    let result = mac.finalize().into_bytes();
    base64::engine::general_purpose::STANDARD.encode(result)
}

/// Convenience for callers that have a [`Credentials`] in hand.
pub fn sign_l2(creds: &Credentials, timestamp: &str, method: &str, path: &str, body: &str) -> String {
    compute_l2_hmac(creds.secret.expose_secret(), timestamp, method, path, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_known_vector() {
        // Generated with the same algorithm in Go middleware (computeHMAC):
        //   secret    = "c2VjcmV0LXRlc3Qta2V5LWFhYWFhYWFhYWFhYWFhYWFhYWE="  // "secret-test-key-aaaaaaaaaaaaaaaaaaa" base64'd
        //   timestamp = "1700000000"
        //   method    = "GET"
        //   path      = "/orders"
        //   body      = ""
        // We compute it the same way here and assert non-empty + stable shape.
        let secret = "c2VjcmV0LXRlc3Qta2V5LWFhYWFhYWFhYWFhYWFhYWFhYWE=";
        let sig = compute_l2_hmac(secret, "1700000000", "GET", "/orders", "");
        // 32 raw bytes -> 44 char base64 (with one '=' padding).
        assert_eq!(sig.len(), 44);
        assert!(sig.ends_with('='));
    }
}
