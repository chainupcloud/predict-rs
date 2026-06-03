//! Response types for the `data-service`. Field shapes match the Go handler structs
//! at the platform repo's `services/data-service/internal/handlers/` 1:1 — including specific
//! additions (`questionTranslation` / `eventTitleTranslation` i18n fields, `fee` on trades,
//! the wrapped leaderboard envelope, `unwrap-requests`).

use serde::{Deserialize, Serialize};

// ─── /positions ────────────────────────────────────────────────────────────

/// Open position row returned by `GET /positions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub proxy_wallet: String,
    pub asset: String,
    pub condition_id: String,
    pub size: f64,
    pub avg_price: f64,
    pub initial_value: f64,
    pub current_value: f64,
    pub cash_pnl: f64,
    pub percent_pnl: f64,
    pub total_bought: f64,
    pub realized_pnl: f64,
    pub percent_realized_pnl: f64,
    pub cur_price: f64,
    pub redeemable: bool,
    pub mergeable: bool,
    pub title: String,
    /// i18n extension — JSON-encoded `{"zh_CN":"...","en_US":"..."}`.
    #[serde(default)]
    pub question_translation: String,
    /// i18n extension.
    #[serde(default)]
    pub event_title_translation: String,
    pub slug: String,
    pub icon: String,
    pub event_slug: String,
    pub outcome: String,
    pub outcome_index: i32,
    pub opposite_outcome: String,
    pub opposite_asset: String,
    pub end_date: String,
    pub negative_risk: bool,
    /// `"negrisk" | "sports" | ""`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub neg_risk_market_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neg_risk_total_options: Option<i32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game_id: String,
}

/// Closed position row returned by `GET /closed-positions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClosedPosition {
    pub proxy_wallet: String,
    pub asset: String,
    pub condition_id: String,
    pub avg_price: f64,
    pub total_bought: f64,
    pub realized_pnl: f64,
    pub cur_price: f64,
    pub timestamp: i64,
    pub title: String,
    pub slug: String,
    pub icon: String,
    pub event_slug: String,
    pub outcome: String,
    pub outcome_index: i32,
    pub opposite_outcome: String,
    pub opposite_asset: String,
    pub end_date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub neg_risk_market_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neg_risk_total_options: Option<i32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game_id: String,
}

/// Per-trader position row inside a [`MarketPositionGroup`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketPositionEntry {
    pub proxy_wallet: String,
    pub name: String,
    #[serde(default)]
    pub profile_image: String,
    #[serde(default)]
    pub verified: bool,
    pub asset: String,
    pub condition_id: String,
    pub avg_price: f64,
    pub size: f64,
    pub curr_price: f64,
    pub current_value: f64,
    pub cash_pnl: f64,
    pub total_bought: f64,
    pub realized_pnl: f64,
    pub total_pnl: f64,
    pub outcome: String,
    pub outcome_index: i32,
}

/// One token bucket returned by `GET /v1/market-positions`. Each bucket carries the token
/// id, the originating condition id, and the per-trader position list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketPositionGroup {
    pub token: String,
    pub condition_id: String,
    #[serde(default)]
    pub positions: Vec<MarketPositionEntry>,
}

// ─── /trades ───────────────────────────────────────────────────────────────

/// Trade row returned by `GET /trades`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub proxy_wallet: String,
    pub side: String,
    pub asset: String,
    pub condition_id: String,
    pub size: f64,
    pub price: f64,
    pub timestamp: i64,
    pub title: String,
    pub slug: String,
    pub icon: String,
    pub event_slug: String,
    pub outcome: String,
    pub outcome_index: i32,
    pub name: String,
    pub pseudonym: String,
    pub bio: String,
    pub profile_image: String,
    pub profile_image_optimized: String,
    pub transaction_hash: String,
    /// Extension field — fee in USDW. Non-omitempty.
    pub fee: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub neg_risk_market_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game_id: String,
}

// ─── /activity ─────────────────────────────────────────────────────────────

/// Activity row returned by `GET /activity`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub proxy_wallet: String,
    pub timestamp: i64,
    pub condition_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub neg_risk_market_id: String,
    /// `TRADE | SPLIT | MERGE | REDEEM | REWARD | CONVERSION`.
    #[serde(rename = "type")]
    pub activity_type: String,
    pub size: f64,
    pub usdc_size: f64,
    pub transaction_hash: String,
    pub price: f64,
    pub asset: String,
    pub side: String,
    pub outcome_index: i32,
    pub title: String,
    #[serde(default)]
    pub question_translation: String,
    #[serde(default)]
    pub event_title_translation: String,
    pub slug: String,
    pub icon: String,
    pub event_slug: String,
    pub outcome: String,
    pub name: String,
    pub pseudonym: String,
    pub bio: String,
    pub profile_image: String,
    pub profile_image_optimized: String,
}

// ─── /holders ──────────────────────────────────────────────────────────────

/// Token-holder entry inside a [`HoldersBucket`]. Live shape verified 2026-05-20.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Holder {
    pub proxy_wallet: String,
    pub asset: String,
    pub amount: f64,
    pub outcome_index: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pseudonym: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub profile_image: String,
    #[serde(default)]
    pub profile_image_optimized: String,
    #[serde(default)]
    pub display_username_public: bool,
}

/// One bucket inside the array returned by `GET /holders` (one bucket per token).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HoldersBucket {
    pub token: String,
    #[serde(default)]
    pub holders: Vec<Holder>,
}

// ─── /traded ───────────────────────────────────────────────────────────────

/// Response from `GET /traded` — count of unique markets traded by a wallet. Wire shape
/// (verified 2026-05-20): `{"traded": <int>, "user": "<addr>"}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TradedResponse {
    pub traded: i32,
    #[serde(default)]
    pub user: String,
}

// ─── /oi (open interest) ──────────────────────────────────────────────────

/// Single row in the array returned by `GET /oi`. Live shape verified 2026-05-20:
/// `[{ "market": "0x...", "value": 10, "scope": "condition" }]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenInterestEntry {
    pub market: String,
    pub value: f64,
    /// `"condition" | "negRiskParent" | "sportsEvent"` etc. — denotes which level the
    /// `market` field is grouped at.
    #[serde(default)]
    pub scope: String,
}

// ─── /live-volume ──────────────────────────────────────────────────────────

/// Single market entry inside a [`LiveVolumeBucket`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveVolumeMarket {
    pub market: String,
    pub value: f64,
}

/// One bucket inside the array returned by `GET /live-volume`. Live shape verified
/// 2026-05-20: top-level is an array of buckets, each one carrying its own `total` + the
/// optional `negRiskMarketId` / `gameId` group key + a `markets` sub-array. neg-risk events
/// yield one bucket per parent; sports events yield one bucket per game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveVolumeBucket {
    pub total: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub neg_risk_market_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game_id: String,
    #[serde(default)]
    pub markets: Vec<LiveVolumeMarket>,
}

// ─── /prices-history ───────────────────────────────────────────────────────

/// Single price point in a [`PricesHistoryResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    /// Unix timestamp in seconds.
    pub t: i64,
    /// Mid-price.
    pub p: f64,
}

/// Response from `GET /prices-history`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PricesHistoryResponse {
    #[serde(default)]
    pub history: Vec<PricePoint>,
}

// ─── /user-pnl ─────────────────────────────────────────────────────────────

/// Single PNL point in a [`UserPnlResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PnlPoint {
    /// Unix timestamp in seconds.
    pub t: i64,
    /// Cumulative profit/loss in USDC.
    pub p: f64,
}

/// Response from `GET /user-pnl`.
pub type UserPnlResponse = Vec<PnlPoint>;

// ─── /stats ────────────────────────────────────────────────────────────────

/// Response from `GET /stats` — global platform statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatsResponse {
    pub total_volume: f64,
    pub volume_24h: f64,
    pub total_trades: i64,
    pub trades_24h: i64,
    pub active_markets: i32,
    pub open_interest: f64,
}

// ─── /v1/leaderboard ──────────────────────────────────────────────────────

/// Single row in [`LeaderboardResponse::data`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardEntry {
    pub rank: String,
    pub proxy_wallet: String,
    pub user_name: String,
    pub profile_image: String,
    pub x_username: String,
    pub verified_badge: bool,
    pub pnl: f64,
    pub vol: f64,
}

/// Single row in [`LeaderboardResponse::biggest_wins`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BiggestWinEntry {
    pub username: String,
    pub avatar: String,
    pub address: String,
    pub title: String,
    pub slug: String,
    pub event_slug: String,
    pub entry_value: f64,
    pub exit_value: f64,
    pub profit: f64,
}

/// Wrapped envelope returned by `GET /v1/leaderboard`. The deviation from upstream V1
/// is intentional: bundling `biggest_wins` into the same response saves a second RTT for the
/// trader-leaderboard UI which renders both columns at once.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardResponse {
    #[serde(default)]
    pub data: Vec<LeaderboardEntry>,
    #[serde(default)]
    pub biggest_wins: Vec<BiggestWinEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

// ─── /unwrap-requests ──────────────────────────────────────────────────────

/// USDW unwrap-request row. No upstream V1 equivalent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UnwrapRequest {
    pub id: String,
    pub request_id: String,
    pub recipient: String,
    pub asset: String,
    pub usdw_amount: String,
    pub asset_amount: String,
    pub claimable_at: String,
    pub claimed: bool,
    pub init_tx_hash: String,
    pub init_timestamp: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claim_tx_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claim_timestamp: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub actual_recipient: String,
}
