//! Prediction market CLOB Rust client SDK.
//!
//! Port of the upstream V1 `rs-clob-client` SDK,
//! adapted for the prediction-market
//! platform. Differences from upstream:
//!
//! - **EIP-712 ClobAuth** is extended with a `bytes32 scopeId` field for multi-tenant isolation.
//! - **EIP-712 Order** struct is extended with a `bytes32 scopeId` field at the end (13 fields total).
//! - **HTTP auth headers** are renamed from `POLY_*` to `PRED_*`.
//! - **HMAC secret encoding** uses standard base64 (this SDK) vs URL-safe (upstream V1).
//!
//! See `pm-sdk-go` for the Go-side counterpart targeting the same backend.

pub mod auth;
pub mod client;
pub mod clob;
pub mod data;
pub mod endpoints;
pub mod error;
pub mod gamma;
pub mod relayer;
pub mod safe;
pub mod signer;
pub mod types;
pub mod ws;

pub use auth::Credentials;
pub use client::{Client, ClientBuilder};
pub use clob::order_builder::{Limit, Market, OrderBuilder};
pub use clob::types::{
    ApiKeyInfo, AssetType, BalanceAllowanceResponse, CancelMarketOrderRequest,
    CancelOrdersResponse, HeartbeatResponse, OpenOrderResponse, OrderScoringResponse, OrderType,
    OrdersRequest, Page, PostOrderResponse, ReplaceOrdersRequest, ReplaceOrdersResponse,
    SendOrderRequest, SignableOrder, SignedOrder, TradeResponse, TradesRequest,
};
pub use clob::ws::{ClobWebSocketClient, MarketStream, MarketSubscribeOpts, UserStream};
pub use data::DataClient;
pub use endpoints::Endpoints;
pub use error::{Error, Result};
pub use gamma::GammaClient;
pub use signer::PMCup26Signer;
pub use types::{Side, SignatureType};

/// Default platform dev endpoint.
pub const DEFAULT_ENDPOINT: &str = "https://clob-api.hermestrade.xyz";

/// OP Sepolia chain ID — the canonical platform staging chain.
pub const OP_SEPOLIA: u64 = 11_155_420;

/// Polygon mainnet chain ID — used by golden test vectors.
pub const POLYGON: u64 = 137;
