//! Wire types for the CLOB REST API. Matches `pm-cup2026/services/clob-service/docs/openapi.yaml`.

use std::collections::HashMap;
use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{DisplayFromStr, serde_as};

use crate::types::{Side, SignatureType};

/// Deserializer helper: the server sometimes returns `null` instead of `[]` for empty list
/// fields (e.g. `associate_trades` on a freshly placed order with no fills, `data` on a
/// `/orders` page with no results). Treat `null` as an empty vec.
fn null_as_empty_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(d).map(Option::unwrap_or_default)
}

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
    /// Fee rate in basis points. Server returns this field as `base_fee`; Polymarket V1 uses
    /// `fee_rate_bps` / `feeRateBps`.
    #[serde(alias = "feeRateBps", alias = "fee_rate_bps", alias = "base_fee")]
    pub fee_rate_bps: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LastTradePriceResponse {
    #[serde(alias = "last_trade_price")]
    pub price: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderBookSummary {
    /// The CLOB token id (uint256 decimal string).
    pub asset_id: String,
    /// Condition id this token belongs to — the server sends this as `market` alongside
    /// `asset_id` in the same payload, so the two are distinct fields, not aliases.
    #[serde(default)]
    pub market: Option<String>,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
    /// Server-specific: last-trade-price echo (decimal string).
    #[serde(default)]
    pub last_trade_price: Option<String>,
    /// Server-specific: per-market tick size returned inside `/book` (decimal string).
    #[serde(default)]
    pub tick_size: Option<String>,
    /// Server-specific: neg-risk flag returned inside `/book`.
    #[serde(default)]
    pub neg_risk: Option<bool>,
    /// Server-specific: minimum order size (decimal string).
    #[serde(default)]
    pub min_order_size: Option<String>,
    /// Server-specific: maximum order size (decimal string). Empty string when uncapped.
    #[serde(default)]
    pub max_order_size: Option<String>,
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
    /// Either "buy" or "sell". Required by the endpoint.
    pub side: Side,
}

#[derive(Clone, Debug, Serialize)]
pub struct SingleTokenRequest {
    pub token_id: String,
}

// ─── Batch-read responses (POST endpoints) ─────────────────────────────────

/// Response shape for `POST /midpoints` — map `token_id -> midpoint` (Decimal, stringified
/// by the server).
pub type MidpointsResponse = HashMap<String, Decimal>;

/// Response shape for `POST /spreads` — map `token_id -> spread` (Decimal stringified).
pub type SpreadsResponse = HashMap<String, Decimal>;

/// Response shape for `POST /prices` — nested map `token_id -> { "BUY": price, "SELL": price }`.
/// The server returns floating-point prices (not strings) for this endpoint.
pub type PricesResponse = HashMap<String, HashMap<String, f64>>;

/// One entry in the `POST /last-trades-prices` response.
#[derive(Clone, Debug, Deserialize)]
pub struct LastTradePriceEntry {
    pub token_id: String,
    pub price: Decimal,
    /// Last trade side as a free-form string (empty when no trades have happened yet).
    #[serde(default)]
    pub side: String,
}

// ─── Price history ─────────────────────────────────────────────────────────

/// `GET /price-history` interval enum. Available intervals; no minute granularity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceHistoryInterval {
    H1,
    H6,
    D1,
    W1,
    M1,
    All,
}

impl PriceHistoryInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::H1 => "1H",
            Self::H6 => "6H",
            Self::D1 => "1D",
            Self::W1 => "1W",
            Self::M1 => "1M",
            Self::All => "ALL",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PricePoint {
    /// Unix-seconds bucket timestamp.
    #[serde(alias = "timestamp")]
    pub t: i64,
    /// Bucket price as a decimal string.
    #[serde(alias = "price")]
    pub p: Decimal,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PriceHistoryResponse {
    #[serde(default)]
    pub history: Vec<PricePoint>,
}

// ─── Internal request items (POST body shapes) ─────────────────────────────

/// Wire shape for batch requests that take only `{ token_id }`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct TokenIdItem<'a> {
    pub token_id: &'a str,
}

/// Wire shape for batch requests that take `{ token_id, side }`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct TokenSideItem {
    pub token_id: String,
    /// "BUY" or "SELL" — the server uppercases anyway, but we send the canonical form.
    pub side: &'static str,
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
/// `proxy_wallet` (Safe address, derived from EOA + scopeId at API-key
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
    /// one key has it stored. Extension field; absent on Polymarket V1.
    #[serde(default, alias = "proxyWallet", alias = "proxy_wallet")]
    pub proxy_wallet: Option<String>,
}

/// Response from `GET /balance-allowance` (L2-authenticated).
///
/// The server automatically derives the Safe address from `EOA + scopeId` and returns the Safe
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
    /// Available balance after subtracting open-order locks. Only set when
    /// the virtual-balance manager is enabled.
    #[serde(default, rename = "virtual_available")]
    pub virtual_available: Option<String>,
    /// Amount locked by open orders. Only set when virtual-balance is enabled.
    #[serde(default)]
    pub locked: Option<String>,
}

// ─── Order / trade / cancel wire types ───────────────────────────

/// Time-in-force / order-type discriminator. Wire form is the bare string `GTC` / `GTD` /
/// `FOK` / `FAK` (matches `services/clob-service/internal/shared/types.OrderType` and the
/// `enum [GTC,GTD,FOK,FAK]` declared in openapi.yaml).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderType {
    /// Good-Til-Cancelled (default for limit orders).
    Gtc,
    /// Good-Til-Date — requires `expiration` at least 60s in the future.
    Gtd,
    /// Fill-Or-Kill — fully fills or rejects (market order).
    Fok,
    /// Fill-And-Kill — best-effort fill, cancel remainder (market order).
    Fak,
}

impl OrderType {
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Gtc => "GTC",
            Self::Gtd => "GTD",
            Self::Fok => "FOK",
            Self::Fak => "FAK",
        }
    }

    /// `true` when the order is a market order (FAK / FOK).
    #[must_use]
    pub fn is_market(self) -> bool {
        matches!(self, Self::Fak | Self::Fok)
    }

    /// `true` when the order is a limit order (GTC / GTD).
    #[must_use]
    pub fn is_limit(self) -> bool {
        matches!(self, Self::Gtc | Self::Gtd)
    }
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

// `Side` (`"BUY"` / `"SELL"`) is re-exported from the crate-level `types` module; it
// already serialises in uppercase, so the wire form here is identical to the enum.

/// JSON wire form of a signed exchange order.
///
/// Matches the Go `handlers.orderJSON` (request body for `POST /order` and `POST /orders`).
/// Every numeric field — `salt`, `tokenID`, `makerAmount`, `takerAmount`, `expiration`,
/// `nonce`, `feeRateBps` — is a decimal string. `side` is `"BUY"` / `"SELL"`,
/// `signatureType` is the string form of the enum (`"0"` / `"1"` / `"2"`), `signature` is
/// `0x` + 130 hex chars, `scopeId` is `0x` + 64 hex (omitted when empty).
///
/// The JSON field name for the token id is **`tokenID`** (mixed case, matches Go), NOT
/// `token_id`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedOrder {
    pub salt: String,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenID")]
    pub token_id: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: String,
    #[serde(rename = "takerAmount")]
    pub taker_amount: String,
    pub expiration: String,
    pub nonce: String,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: String,
    pub side: Side,
    #[serde(rename = "signatureType")]
    pub signature_type: String,
    pub signature: String,
    /// Hex `0x` + 64-char scope id. Empty string when no scope binding is set.
    #[serde(rename = "scopeId", default, skip_serializing_if = "String::is_empty")]
    pub scope_id: String,
}

impl SignedOrder {
    /// `signatureType` is serialised on the wire as the numeric string (`"0"` / `"1"` / `"2"`).
    /// Convenience to construct from the typed enum.
    #[must_use]
    pub fn signature_type_enum(&self) -> Option<SignatureType> {
        match self.signature_type.as_str() {
            "0" => Some(SignatureType::Eoa),
            "1" => Some(SignatureType::PolyProxy),
            "2" => Some(SignatureType::PolyGnosisSafe),
            _ => None,
        }
    }
}

/// In-memory companion to [`SignedOrder`] — the typed view callers see *before* serialisation.
/// `build_and_sign` on [`crate::clob::order_builder::OrderBuilder`] returns a [`SignedOrder`];
/// `build` returns a `SignableOrder` (unsigned) so callers can attach a signature externally.
#[derive(Clone, Debug)]
pub struct SignableOrder {
    /// Plain-data fields ready for the EIP-712 signer.
    pub order: crate::signer::OrderForSigning,
    /// Time-in-force (forwarded as `orderType` on the outer envelope).
    pub order_type: OrderType,
    /// `postOnly` flag on the outer envelope (limit orders only).
    pub post_only: bool,
    /// Optional `owner` UUID. Empty = server uses the API-key owner.
    pub owner: String,
}

/// `POST /order` and `POST /orders` request envelope: matches `handlers.orderRequest`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendOrderRequest {
    pub order: SignedOrder,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(rename = "orderType")]
    pub order_type: OrderType,
    #[serde(rename = "postOnly", default, skip_serializing_if = "is_false")]
    pub post_only: bool,
    /// Reserved flag for matched-but-defer-execution behaviour. Always
    /// false from this SDK.
    #[serde(rename = "deferExec", default, skip_serializing_if = "is_false")]
    pub defer_exec: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// `POST /orders/replace` request body.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ReplaceOrdersRequest {
    #[serde(rename = "cancelOrderIDs")]
    pub cancel_order_ids: Vec<String>,
    pub orders: Vec<SendOrderRequest>,
}

/// `POST /order` (and per-item batch) response.
#[derive(Clone, Debug, Deserialize, Default)]
pub struct PostOrderResponse {
    #[serde(default)]
    pub success: bool,
    #[serde(default, rename = "errorMsg")]
    pub error_msg: String,
    #[serde(default, rename = "orderID")]
    pub order_id: String,
    #[serde(default, rename = "takingAmount")]
    pub taking_amount: String,
    #[serde(default, rename = "makingAmount")]
    pub making_amount: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "transactionsHashes")]
    pub transactions_hashes: Vec<String>,
    #[serde(default, rename = "tradeIDs", alias = "tradeIds")]
    pub trade_ids: Vec<String>,
}

/// `DELETE /order` / `DELETE /orders` / `DELETE /cancel-all` / `DELETE /cancel-market-orders`
/// response.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CancelOrdersResponse {
    #[serde(default)]
    pub canceled: Vec<String>,
    /// Map of `orderID -> reason`. Server uses `{}` for fully-successful cancellation.
    #[serde(default, rename = "not_canceled")]
    pub not_canceled: HashMap<String, String>,
}

/// `DELETE /cancel-market-orders` request body. At least one of `market` (condition id)
/// or `asset_id` (token id) must be present.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CancelMarketOrderRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
    #[serde(default, rename = "asset_id", skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
}

impl CancelMarketOrderRequest {
    #[must_use]
    pub fn by_market(condition_id: impl Into<String>) -> Self {
        Self {
            market: Some(condition_id.into()),
            asset_id: None,
        }
    }

    #[must_use]
    pub fn by_asset(token_id: impl Into<String>) -> Self {
        Self {
            market: None,
            asset_id: Some(token_id.into()),
        }
    }
}

/// `GET /orders` query parameters. Builder-friendly — every field is optional.
#[derive(Clone, Debug, Default)]
pub struct OrdersRequest {
    pub id: Option<String>,
    pub market: Option<String>,
    pub asset_id: Option<String>,
    /// `ORDER_STATUS_LIVE` (default), `"all"`, or an explicit `OrderStatus` literal.
    pub status: Option<String>,
}

/// `GET /trades` query parameters. `maker_address` is server-required; the SDK fills it
/// from the configured signer if the caller leaves it unset.
#[derive(Clone, Debug, Default)]
pub struct TradesRequest {
    pub maker_address: Option<String>,
    pub id: Option<String>,
    pub market: Option<String>,
    pub asset_id: Option<String>,
    /// Unix-seconds upper bound.
    pub before: Option<i64>,
    /// Unix-seconds lower bound.
    pub after: Option<i64>,
    /// Snowflake `from_id` ASC cursor.
    pub from_id: Option<i64>,
    /// Page size [1, 1000]. Server default 100.
    pub limit: Option<u32>,
}

/// One row in `GET /orders` (`openOrderJSON` on the server).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct OpenOrderResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default, rename = "maker_address")]
    pub maker_address: String,
    #[serde(default)]
    pub market: String,
    #[serde(default, rename = "asset_id")]
    pub asset_id: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default, rename = "original_size")]
    pub original_size: String,
    #[serde(default, rename = "size_matched")]
    pub size_matched: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub expiration: String,
    #[serde(default, rename = "order_type")]
    pub order_type: String,
    #[serde(default, rename = "created_at")]
    pub created_at: String,
    #[serde(default, rename = "associate_trades", deserialize_with = "null_as_empty_vec")]
    pub associate_trades: Vec<String>,
    #[serde(default)]
    pub lazy: bool,
}

/// One row in `GET /trades` (`tradeJSON` on the server).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TradeResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "taker_order_id")]
    pub taker_order_id: String,
    #[serde(default)]
    pub market: String,
    #[serde(default, rename = "asset_id")]
    pub asset_id: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub size: String,
    #[serde(default, rename = "fee_rate_bps")]
    pub fee_rate_bps: String,
    #[serde(default)]
    pub fee: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "match_time")]
    pub match_time: String,
    #[serde(default, rename = "match_time_nano")]
    pub match_time_nano: String,
    #[serde(default, rename = "last_update")]
    pub last_update: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default, rename = "bucket_index")]
    pub bucket_index: i64,
    #[serde(default)]
    pub owner: String,
    #[serde(default, rename = "maker_address")]
    pub maker_address: String,
    #[serde(default, rename = "transaction_hash")]
    pub transaction_hash: String,
    #[serde(default, rename = "trader_side")]
    pub trader_side: String,
    #[serde(default, rename = "maker_orders")]
    pub maker_orders: Vec<serde_json::Value>,
    #[serde(default, rename = "match_type")]
    pub match_type: String,
    #[serde(default, rename = "order_type")]
    pub order_type: String,
}

/// Cursor-paginated envelope used by `GET /orders` and `GET /trades`.
///
/// The server signals end-of-stream by emitting `next_cursor: "LTE="` (the base64 encoding of
/// `"-1"`) — see [`Page::END_CURSOR`]. The empty string is also treated as end-of-stream for
/// resilience.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    /// Page-size echoed by the server.
    #[serde(default)]
    pub limit: u32,
    /// Number of items in `data`.
    #[serde(default)]
    pub count: u32,
    /// Opaque cursor for the next page; `"LTE="` means no more data.
    #[serde(default, rename = "next_cursor")]
    pub next_cursor: String,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub data: Vec<T>,
}

impl<T> Page<T> {
    /// Server-side sentinel for "no more pages".
    pub const END_CURSOR: &'static str = "LTE=";

    /// `true` when `next_cursor` is empty or `"LTE="`.
    #[must_use]
    pub fn is_end(&self) -> bool {
        self.next_cursor.is_empty() || self.next_cursor == Self::END_CURSOR
    }
}

/// `GET /order-scoring` response. Server currently always returns `{ "scoring": true }`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct OrderScoringResponse {
    #[serde(default)]
    pub scoring: bool,
}

/// `POST /heartbeats` response.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct HeartbeatResponse {
    #[serde(default)]
    pub status: String,
}

/// `POST /orders/replace` per-cancel result.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReplaceCancelResult {
    #[serde(default, rename = "orderID")]
    pub order_id: String,
    #[serde(default)]
    pub status: String,
}

/// `POST /orders/replace` per-placement result.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReplacePlaceResult {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub success: bool,
    #[serde(default, rename = "errorMsg")]
    pub error_msg: String,
    #[serde(default, rename = "orderID")]
    pub order_id: String,
    #[serde(default, rename = "takingAmount")]
    pub taking_amount: String,
    #[serde(default, rename = "makingAmount")]
    pub making_amount: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "transactionsHashes")]
    pub transactions_hashes: Vec<String>,
    #[serde(default, rename = "tradeIDs", alias = "tradeIds")]
    pub trade_ids: Vec<String>,
}

/// `POST /orders/replace` response.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReplaceOrdersResponse {
    #[serde(default, rename = "stoppedAt")]
    pub stopped_at: String,
    #[serde(default)]
    pub cancels: Vec<ReplaceCancelResult>,
    #[serde(default)]
    pub placements: Vec<ReplacePlaceResult>,
    #[serde(default, rename = "errorMsg")]
    pub error_msg: String,
}
