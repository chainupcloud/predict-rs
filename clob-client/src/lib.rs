//! ChainUp pm-cup2026 CLOB Rust client SDK.
//!
//! Port of Polymarket's [`rs-clob-client`](https://github.com/Polymarket/rs-clob-client) V1 SDK,
//! adapted for ChainUp's [`pm-cup2026`](https://github.com/chainupcloud/pm-cup2026) prediction-market
//! platform. Differences from upstream:
//!
//! - **EIP-712 ClobAuth** is extended with a `bytes32 scopeId` field for multi-tenant isolation.
//! - **EIP-712 Order** struct is extended with a `bytes32 scopeId` field at the end (13 fields total).
//! - **HTTP auth headers** are renamed from `POLY_*` to `PRED_*`.
//! - **HMAC secret encoding** uses standard base64 (chainup) vs URL-safe (Polymarket).
//!
//! See `pm-sdk-go` for the Go-side counterpart targeting the same backend.

pub mod auth;
pub mod client;
pub mod clob;
pub mod endpoints;
pub mod error;
pub mod gamma;
pub mod signer;
pub mod types;

pub use client::{Client, ClientBuilder};
pub use endpoints::Endpoints;
pub use error::{Error, Result};
pub use gamma::GammaClient;

/// Default `pm-cup2026` dev endpoint.
pub const DEFAULT_ENDPOINT: &str = "https://clob-api.predict.prax1s.xyz";

/// OP Sepolia chain ID — the canonical pm-cup2026 staging chain.
pub const OP_SEPOLIA: u64 = 11_155_420;

/// Polygon mainnet chain ID — used by golden test vectors.
pub const POLYGON: u64 = 137;
