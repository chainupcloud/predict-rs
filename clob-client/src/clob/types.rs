//! Wire types for the chainup CLOB REST API. Matches `pm-cup2026/services/clob-service/docs/openapi.yaml`.

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
