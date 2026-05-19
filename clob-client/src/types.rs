//! Common domain types: address aliases, side, signature type, scope id.

use serde::{Deserialize, Serialize};

pub use alloy::primitives::{Address, B256, U256};

/// `bytes32` multi-tenant scope identifier. Zero = no scope binding.
///
/// Serialized as a hex string with `0x` prefix (empty string for zero on the wire);
/// see [`ScopeId::to_hex`] / [`ScopeId::from_hex`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScopeId(pub [u8; 32]);

impl ScopeId {
    pub const ZERO: ScopeId = ScopeId([0u8; 32]);

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }

    /// Parse from hex string ("0x..." or ""). Empty -> zero.
    /// Mirrors `pm-sdk-go`'s `ScopeIDFromHex`: pads short input on the right side (left-pad with zeros).
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        if s.is_empty() {
            return Ok(Self::ZERO);
        }
        let stripped = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(stripped)?;
        let mut out = [0u8; 32];
        let n = bytes.len().min(32);
        out[32 - n..].copy_from_slice(&bytes[..n]);
        Ok(Self(out))
    }

    /// Format as "0x..." hex. Returns empty string for zero (matches pm-sdk-go's `ScopeIDToHex`).
    #[must_use]
    pub fn to_hex(&self) -> String {
        if self.is_zero() {
            String::new()
        } else {
            format!("0x{}", hex::encode(self.0))
        }
    }

    #[must_use]
    pub fn as_b256(&self) -> B256 {
        B256::from(self.0)
    }
}

impl From<[u8; 32]> for ScopeId {
    fn from(b: [u8; 32]) -> Self {
        Self(b)
    }
}

impl From<B256> for ScopeId {
    fn from(b: B256) -> Self {
        Self(b.0)
    }
}

/// Order side.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Buy = 0,
    Sell = 1,
}

impl Side {
    #[must_use]
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Polymarket-style signature type — same numeric values as pm-cup2026.
///
/// - `Eoa` (0): direct EOA signature
/// - `PolyProxy` (1): Polymarket proxy wallet (Magic / email login)
/// - `PolyGnosisSafe` (2): browser wallet via Gnosis Safe (1-of-1) — chainup default
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SignatureType {
    Eoa = 0,
    PolyProxy = 1,
    PolyGnosisSafe = 2,
}

impl SignatureType {
    #[must_use]
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl std::fmt::Display for SignatureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eoa => f.write_str("EOA"),
            Self::PolyProxy => f.write_str("POLY_PROXY"),
            Self::PolyGnosisSafe => f.write_str("POLY_GNOSIS_SAFE"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_id_roundtrip() {
        let s = "0x0000000000000000000000000000000000000000000000000000000000000042";
        let parsed = ScopeId::from_hex(s).unwrap();
        assert_eq!(parsed.0[31], 0x42);
        assert_eq!(parsed.to_hex(), s);
    }

    #[test]
    fn scope_id_empty_is_zero() {
        let parsed = ScopeId::from_hex("").unwrap();
        assert!(parsed.is_zero());
        assert_eq!(parsed.to_hex(), "");
    }

    #[test]
    fn scope_id_left_pads_short_input() {
        // "0x42" should pad to ...0042
        let parsed = ScopeId::from_hex("0x42").unwrap();
        assert_eq!(parsed.0[31], 0x42);
        for b in &parsed.0[..31] {
            assert_eq!(*b, 0);
        }
    }
}
