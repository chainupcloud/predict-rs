//! Wire types for the chainup CLOB REST API. Matches `pm-cup2026/services/clob-service/docs/openapi.yaml`.

use std::collections::HashMap;
use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

use crate::types::Side;

// ─── Public market-data responses ───────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct MidpointResponse {
    #[serde(alias = "mid")]
    pub price: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PriceResponse {
    pub price: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpreadResponse {
    pub spread: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TickSizeResponse {
    /// Minimum tick size, as a decimal string (e.g. "0.01"). The server returns this as a string
    /// for some endpoints and as a number for others; both are accepted via the deserializer.
    #[serde(alias = "minimum_tick_size")]
    pub minimum_tick_size: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FeeRateResponse {
    #[serde(alias = "feeRateBps", alias = "fee_rate_bps")]
    pub fee_rate_bps: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LastTradePriceResponse {
    #[serde(alias = "last_trade_price")]
    pub price: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderBookSummary {
    #[serde(alias = "market", alias = "asset_id")]
    pub asset_id: String,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    #[serde(default, alias = "timestamp")]
    pub timestamp: Option<String>,
    #[serde(default, alias = "hash")]
    pub hash: Option<String>,
}

#[serde_as]
#[derive(Clone, Debug, Deserialize)]
pub struct OrderBookLevel {
    #[serde_as(as = "DisplayFromStr")]
    pub price: Decimal,
    #[serde_as(as = "DisplayFromStr")]
    pub size: Decimal,
}

// ─── Query / request structs ────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
pub struct PriceRequest {
    pub token_id: String,
    /// Either "buy" or "sell". Required by the chainup endpoint.
    pub side: Side,
}

#[derive(Clone, Debug, Serialize)]
pub struct SingleTokenRequest {
    pub token_id: String,
}

// ─── Auth / balance-allowance ────────────────────────────────────────────

/// `asset_type` query parameter for `/balance-allowance` and `/balance-allowance/update`.
///
/// Mirrors the server-side `handlers.GetBalanceAllowance`: only the string literals
/// `"COLLATERAL"` and `"CONDITIONAL"` are accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AssetType {
    /// USDC (or chain-equivalent collateral) — `token_id` must be omitted.
    Collateral,
    /// Conditional outcome token — `token_id` is required.
    Conditional,
}

impl AssetType {
    /// String value matching the server-side `c.Query("asset_type")` check.
    #[must_use]
    pub fn as_query_str(self) -> &'static str {
        match self {
            Self::Collateral => "COLLATERAL",
            Self::Conditional => "CONDITIONAL",
        }
    }
}

impl fmt::Display for AssetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_query_str())
    }
}

/// Response from `GET /auth/api-keys` (L2-authenticated).
///
/// chainup-specific: `proxy_wallet` (Safe address, derived from EOA + scopeId at API-key
/// creation time) is returned alongside the key list.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ApiKeyInfo {
    /// Active API key UUIDs for the authenticated address.
    #[serde(rename = "apiKeys", default)]
    pub api_keys: Vec<String>,
    /// EOA address — the L1 signer behind every listed key.
    #[serde(default)]
    pub address: Option<String>,
    /// Safe wallet address (CREATE2-derived from `signer + scopeId`). Present when at least
    /// one key has it stored. chainup-specific field; absent on Polymarket V1.
    #[serde(default, alias = "proxyWallet", alias = "proxy_wallet")]
    pub proxy_wallet: Option<String>,
}

/// Response from `GET /balance-allowance` (L2-authenticated).
///
/// chainup automatically derives the Safe address from `EOA + scopeId` and returns the Safe
/// wallet's balance, not the EOA's. When the server-side virtual-balance manager is
/// enabled the response also includes `virtual_available` and `locked`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BalanceAllowanceResponse {
    /// On-chain balance returned by the server (raw string — may be a wei-style integer
    /// or a human-readable decimal depending on the asset type).
    #[serde(default)]
    pub balance: String,
    /// Allowance map keyed by spender address. May be empty when the server has no
    /// `onchain.Client` configured.
    #[serde(default)]
    pub allowances: HashMap<String, String>,
    /// chainup-specific: available balance after subtracting open-order locks. Only set when
    /// the virtual-balance manager is enabled.
    #[serde(default, rename = "virtual_available")]
    pub virtual_available: Option<String>,
    /// chainup-specific: amount locked by open orders. Only set when virtual-balance is enabled.
    #[serde(default)]
    pub locked: Option<String>,
}
