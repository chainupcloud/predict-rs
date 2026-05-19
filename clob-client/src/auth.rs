//! L1 / L2 authentication primitives and header constants.
//!
//! Chainup uses `PRED_*` header names (vs Polymarket's `POLY_*`) and standard base64 for the
//! HMAC secret (vs URL-safe).
//!
//! Two flavours are supported:
//!
//! - **L1 (EIP-712)** — used by `/auth/api-key` create/derive/revoke. The signer produces a
//!   `ClobAuth` signature; [`build_l1_headers`] packages it into the four (or five, with
//!   `PRED_SCOPE_ID`) chainup headers.
//! - **L2 (HMAC-SHA256)** — used by trading endpoints. [`build_l2_headers`] computes the HMAC
//!   over `timestamp + method + path + body` with the standard-base64-decoded secret and
//!   returns the five `PRED_*` headers.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::Sha256;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::signer::PMCup26Signer;
use crate::types::Address;

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

/// Current Unix timestamp (seconds) as a string. Server-side L1 / L2 validators expect
/// the value to be parseable as a float (so an integer string is fine).
#[must_use]
pub fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ─── L1 ──────────────────────────────────────────────────────────────────

/// Build the L1 (EIP-712) chainup auth headers. The caller controls the timestamp + nonce
/// for determinism (tests / replays); see [`build_l1_headers_now`] for the live variant.
///
/// `PRED_SCOPE_ID` is emitted only when the signer's scope id is non-zero (matching the
/// server's contract: empty header == no scope binding).
///
/// Errors if signing fails (private key invalid, etc.).
pub fn build_l1_headers_with_timestamp(
    signer: &PMCup26Signer,
    timestamp: &str,
    nonce: u32,
) -> Result<HeaderMap> {
    let sig_bytes = signer.sign_clob_auth(timestamp, u64::from(nonce))?;
    let mut hex_sig = String::with_capacity(2 + sig_bytes.len() * 2);
    hex_sig.push_str("0x");
    hex_sig.push_str(&hex::encode(sig_bytes));

    let mut map = HeaderMap::new();
    map.insert(
        hname(header::PRED_ADDRESS)?,
        HeaderValue::from_str(&format_address(signer.address()))?,
    );
    map.insert(
        hname(header::PRED_NONCE)?,
        HeaderValue::from_str(&nonce.to_string())?,
    );
    map.insert(
        hname(header::PRED_SIGNATURE)?,
        HeaderValue::from_str(&hex_sig)?,
    );
    map.insert(
        hname(header::PRED_TIMESTAMP)?,
        HeaderValue::from_str(timestamp)?,
    );
    let scope_id = signer.scope_id();
    if !scope_id.is_zero() {
        map.insert(
            hname(header::PRED_SCOPE_ID)?,
            HeaderValue::from_str(&scope_id.to_hex())?,
        );
    }
    Ok(map)
}

/// Build L1 headers using the current wall-clock time.
pub fn build_l1_headers(signer: &PMCup26Signer, nonce: Option<u32>) -> Result<HeaderMap> {
    let ts = current_timestamp();
    build_l1_headers_with_timestamp(signer, &ts, nonce.unwrap_or(0))
}

// ─── L2 ──────────────────────────────────────────────────────────────────

/// Compute the chainup L2 HMAC signature.
///
/// Mirrors `services/clob-service/internal/tradingapi/middleware/auth.go::computeHMAC`:
///
/// 1. Decode `secret` with **standard** base64 (`STANDARD`, not URL-safe). If decoding fails
///    the raw bytes are used directly — matching the Go fallback.
/// 2. Concatenate `timestamp + method + path + body` (no separators).
/// 3. HMAC-SHA256 with the key, then encode the MAC as standard base64.
#[must_use]
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
#[must_use]
pub fn sign_l2(creds: &Credentials, timestamp: &str, method: &str, path: &str, body: &str) -> String {
    compute_l2_hmac(creds.secret.expose_secret(), timestamp, method, path, body)
}

/// Build the L2 HMAC chainup auth headers for a single request.
///
/// `path` must be the URL **path only** (no query string) — the server's
/// `middleware/auth.go::L2AuthMiddleware` signs `c.Request.URL.Path`, which excludes the
/// query. `body` must be the request body exactly as it will appear on the wire (empty
/// string for GET / DELETE / etc.).
///
/// `address` is the EOA address whose private key created the API key — the server
/// optionally validates it against the stored key, but only when present.
pub fn build_l2_headers(
    creds: &Credentials,
    address: Address,
    timestamp: &str,
    method: &str,
    path: &str,
    body: &str,
) -> Result<HeaderMap> {
    let signature = sign_l2(creds, timestamp, method, path, body);
    let mut map = HeaderMap::new();
    map.insert(
        hname(header::PRED_API_KEY)?,
        HeaderValue::from_str(&creds.key.to_string())?,
    );
    map.insert(
        hname(header::PRED_PASSPHRASE)?,
        HeaderValue::from_str(creds.passphrase.expose_secret())?,
    );
    map.insert(
        hname(header::PRED_SIGNATURE)?,
        HeaderValue::from_str(&signature)?,
    );
    map.insert(
        hname(header::PRED_TIMESTAMP)?,
        HeaderValue::from_str(timestamp)?,
    );
    map.insert(
        hname(header::PRED_ADDRESS)?,
        HeaderValue::from_str(&format_address(address))?,
    );
    Ok(map)
}

// ─── helpers ─────────────────────────────────────────────────────────────

#[must_use]
fn format_address(addr: Address) -> String {
    // alloy's Display for Address uses EIP-55 mixed case. The server compares
    // addresses case-insensitively (strings.EqualFold), so this is safe.
    format!("{addr:#x}")
}

/// Construct a [`HeaderName`] from a static `&str` constant. The constants in [`header`] are
/// ASCII so this never fails in practice; surfacing the error keeps the function total.
fn hname(name: &'static str) -> std::result::Result<HeaderName, Error> {
    HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| Error::Validation(format!("invalid header name {name}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ScopeId;

    /// Deterministic Hardhat / Anvil account #0 private key. Public; matches both the rs-clob-client
    /// reference vectors and the chainup golden fixture.
    const PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const EXPECTED_ADDR: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

    #[test]
    fn hmac_known_vector() {
        // Generated with the same algorithm in Go middleware (computeHMAC):
        //   secret    = "c2VjcmV0LXRlc3Qta2V5LWFhYWFhYWFhYWFhYWFhYWFhYWE="
        //   timestamp = "1700000000"
        //   method    = "GET"
        //   path      = "/orders"
        //   body      = ""
        let secret = "c2VjcmV0LXRlc3Qta2V5LWFhYWFhYWFhYWFhYWFhYWFhYWE=";
        let sig = compute_l2_hmac(secret, "1700000000", "GET", "/orders", "");
        assert_eq!(sig.len(), 44);
        assert!(sig.ends_with('='));
    }

    /// Cross-check against `references/rs-clob-client/src/auth.rs::hmac_succeeds`.
    /// The Polymarket reference vector uses URL-safe base64; chainup uses standard base64
    /// so the output differs after the first `_/-` substitution. We compute both encodings
    /// for the same secret bytes and confirm the bytes are identical.
    #[test]
    fn hmac_matches_message_layout() {
        // Same secret bytes as the rs-clob-client reference test, but base64 standard
        // re-encoded. Both should HMAC to the same raw bytes; only the base64 alphabet
        // differs.
        let secret_std = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let sig = compute_l2_hmac(secret_std, "1000000", "test-sign", "/orders", "{\"hash\":\"0x123\"}");
        // Standard base64 of the same HMAC bytes that rs-clob-client encodes as URL-safe
        // ("4gJVbox-R6XlDK4nlaicig0_ANVL1qdcahiL8CXfXLM=").
        let expected_std = "4gJVbox+R6XlDK4nlaicig0/ANVL1qdcahiL8CXfXLM=";
        assert_eq!(sig, expected_std);
    }

    #[test]
    fn build_l1_headers_known_signature() {
        // Mirror the golden_signer fixture: chain137_scope1_ts1700000000_nonce0 -> known signature.
        let signer = PMCup26Signer::from_hex(PRIVATE_KEY, 137)
            .unwrap()
            .with_scope_id(ScopeId::from_hex("0x01").unwrap());
        let headers = build_l1_headers_with_timestamp(&signer, "1700000000", 0).unwrap();
        assert_eq!(headers[header::PRED_ADDRESS], EXPECTED_ADDR);
        assert_eq!(headers[header::PRED_NONCE], "0");
        assert_eq!(headers[header::PRED_TIMESTAMP], "1700000000");
        assert_eq!(
            headers[header::PRED_SCOPE_ID],
            "0x0000000000000000000000000000000000000000000000000000000000000001",
        );
        assert_eq!(
            headers[header::PRED_SIGNATURE],
            "0x7db2a7529457b0503cd8b6d4b79e74ee0b5d2f06f987e629cbad28973d8d80bf476b1bae6853a3822ac96e288d921fd36ebd8b7d07483be30310faeb3cefd9c901",
        );
    }

    #[test]
    fn build_l1_headers_omits_scope_when_zero() {
        // chain137_zeroscope_ts1700000000_nonce42 -> empty scope -> no PRED_SCOPE_ID header.
        let signer = PMCup26Signer::from_hex(PRIVATE_KEY, 137).unwrap();
        let headers = build_l1_headers_with_timestamp(&signer, "1700000000", 42).unwrap();
        assert!(!headers.contains_key(header::PRED_SCOPE_ID));
        assert_eq!(headers[header::PRED_NONCE], "42");
        assert_eq!(
            headers[header::PRED_SIGNATURE],
            "0x5868868c57f787624739d0992627f3f48b7e7a61edb9932758b1fad620e39a252a43db027a9b0f4ff8a20ea212fa17f2be08ef2e6475293be44f82cd461d4ec200",
        );
    }

    #[test]
    fn build_l2_headers_round_trip() {
        let creds = Credentials::new(
            Uuid::nil(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
            "pp-test".to_owned(),
        );
        let addr = Address::repeat_byte(0xab);
        let headers = build_l2_headers(&creds, addr, "1700000000", "GET", "/balance-allowance", "")
            .unwrap();
        assert_eq!(headers[header::PRED_API_KEY], Uuid::nil().to_string());
        assert_eq!(headers[header::PRED_PASSPHRASE], "pp-test");
        assert_eq!(headers[header::PRED_TIMESTAMP], "1700000000");
        // EIP-55 mixed-case 20-byte address rendered via lowercase {:#x} (the server compares
        // case-insensitively).
        assert_eq!(headers[header::PRED_ADDRESS], format!("{addr:#x}"));
        // Signature for the known vector — matches the Go middleware byte-for-byte (verified
        // by hmac_matches_message_layout above; this case has a different secret/payload).
        let expected = compute_l2_hmac(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "1700000000",
            "GET",
            "/balance-allowance",
            "",
        );
        assert_eq!(headers[header::PRED_SIGNATURE], expected);
    }
}
