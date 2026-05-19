//! Response types for the chainup Gamma API.
//!
//! Field shapes mirror `pm-cup2026/services/gamma-service/internal/models/models.go` byte-for-byte:
//! - Required string IDs are `String` (never `Option`).
//! - JSON keys use camelCase via `#[serde(rename_all = "camelCase")]`, with
//!   explicit `#[serde(rename = "...")]` for off-pattern fields (`tagID`,
//!   `relatedTagID`, `commentID`, `parentEntityID`, `parentCommentID`).
//! - Almost every other field is nullable on the wire (Go `*T` pointers),
//!   surfaced as `Option<T>` here.
//!
//! Field-set is a superset of what Polymarket Gamma returns — chainup adds
//! `questionTranslation` / `outcomeTranslation` / `titleTranslation` (i18n),
//! `adjudication` (UMA oracle lifecycle), `marketMakerAddress` and `clobTokenIds`
//! as plain JSON-array strings, etc. We keep the wire types thin (no derived
//! fields) and stay tolerant of unknown fields so a server-side addition does
//! not break older client builds.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ─── Pagination + image optimisation ────────────────────────────────────────

/// `Pagination` block embedded in `Search` responses.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub total_results: i64,
}

/// CDN-optimised image metadata embedded in `Profile`, `Comment`, etc.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOptimization {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub image_url_source: Option<String>,
    #[serde(default)]
    pub image_url_optimized: Option<String>,
    #[serde(default)]
    pub image_size_kb_source: Option<f64>,
    #[serde(default)]
    pub image_size_kb_optimized: Option<f64>,
    #[serde(default)]
    pub image_optimized_complete: Option<bool>,
    #[serde(default)]
    pub image_optimized_last_updated: Option<String>,
    #[serde(default, rename = "relID")]
    pub rel_id: Option<i64>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub relname: Option<String>,
}

// ─── Tags ───────────────────────────────────────────────────────────────────

/// A `Tag` used to categorise events and markets.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub label_translation: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub is_carousel: Option<bool>,
    #[serde(default)]
    pub tag_type: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub created_by: Option<i64>,
    #[serde(default)]
    pub updated_by: Option<i64>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// A relationship between two tags (`/tags/{id}/related-tags`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedTag {
    pub id: String,
    #[serde(default, rename = "tagID")]
    pub tag_id: Option<i64>,
    #[serde(default, rename = "relatedTagID")]
    pub related_tag_id: Option<i64>,
    #[serde(default)]
    pub rank: Option<i64>,
}

/// Tag-shape returned by `/public-search` (slim).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SearchTag {
    pub id: String,
    pub label: String,
    pub slug: String,
    #[serde(default)]
    pub event_count: i64,
}

// ─── Categories / chats / templates / creators ──────────────────────────────

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub parent_category: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCreator {
    pub id: String,
    #[serde(default)]
    pub creator_name: Option<String>,
    #[serde(default)]
    pub creator_handle: Option<String>,
    #[serde(default)]
    pub creator_url: Option<String>,
    #[serde(default)]
    pub creator_image: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub channel_name: Option<String>,
    #[serde(default)]
    pub channel_image: Option<String>,
    #[serde(default)]
    pub live: Option<bool>,
    #[serde(default)]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    #[serde(default)]
    pub event_title: Option<String>,
    #[serde(default)]
    pub event_slug: Option<String>,
    #[serde(default)]
    pub event_image: Option<String>,
    #[serde(default)]
    pub market_title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub resolution_source: Option<String>,
    #[serde(default)]
    pub neg_risk: Option<bool>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub show_market_images: Option<bool>,
    #[serde(default)]
    pub series_slug: Option<String>,
    #[serde(default)]
    pub outcomes: Option<String>,
}

// ─── Adjudication (UMA oracle lifecycle, chainup-specific) ──────────────────

/// A single possible next action in the adjudication lifecycle.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NextStep {
    pub action: String,
    #[serde(default)]
    pub deadline: Option<String>,
    pub description: String,
}

/// Oracle adjudication lifecycle and result for a market condition.
///
/// chainup-specific; not present in Polymarket Gamma. Surfaced so the user-dapp
/// dispute flow can read adapter data without indexing chain events.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Adjudication {
    pub status: String,
    pub current_phase: String,
    #[serde(default)]
    pub next_steps: Vec<NextStep>,
    #[serde(default)]
    pub proposed_outcome: Option<String>,
    #[serde(default)]
    pub proposed_at: Option<String>,
    #[serde(default)]
    pub proposer: Option<String>,
    #[serde(default)]
    pub propose_deadline: Option<String>,
    #[serde(default)]
    pub liveness_secs: u64,
    #[serde(default)]
    pub liveness_deadline: Option<String>,
    #[serde(default)]
    pub challenger: Option<String>,
    #[serde(default)]
    pub challenged_at: Option<String>,
    #[serde(default)]
    pub arbitrator: Option<String>,
    #[serde(default)]
    pub arbitrated_at: Option<String>,
    #[serde(default)]
    pub arbitration_correct: Option<i64>,
    #[serde(default)]
    pub settled_outcome: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub payout_vector: Option<String>,
    #[serde(default)]
    pub canceled_at: Option<String>,
    #[serde(default)]
    pub requested_at: Option<String>,
    #[serde(default)]
    pub reset_count: i64,
    #[serde(default)]
    pub question_id: String,
    #[serde(default)]
    pub adapter_address: String,
}

// ─── Markets ────────────────────────────────────────────────────────────────

/// A single binary prediction market. Mirrors `gamma-service/internal/models.Market`.
///
/// Note: `clob_token_ids` is a JSON-array *string* (e.g. `"[\"123\",\"456\"]"`).
/// Callers must `serde_json::from_str` it; see `parse_clob_token_ids` helper on
/// the SDK once you need both yes/no token ids.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub id: String,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub question_translation: Option<String>,
    #[serde(default)]
    pub condition_id: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub resolution_source: Option<String>,
    #[serde(default)]
    pub end_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub liquidity: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub outcomes: Option<String>,
    #[serde(default)]
    pub outcome_translation: Option<String>,
    #[serde(default)]
    pub outcome_prices: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub closed: Option<bool>,
    #[serde(default)]
    pub market_maker_address: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub featured: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub restricted: Option<bool>,
    #[serde(default)]
    pub enable_order_book: Option<bool>,
    #[serde(default)]
    pub order_price_min_tick_size: Option<Decimal>,
    #[serde(default)]
    pub order_min_size: Option<Decimal>,
    #[serde(default)]
    pub order_max_size: Option<Decimal>,
    #[serde(default)]
    pub volume24hr: Option<Decimal>,
    #[serde(default)]
    pub clob_token_ids: Option<String>,
    #[serde(default)]
    pub accepting_orders: Option<bool>,
    #[serde(default)]
    pub last_trade_price: Option<Decimal>,
    #[serde(default)]
    pub best_bid: Option<Decimal>,
    #[serde(default)]
    pub best_ask: Option<Decimal>,
    #[serde(default)]
    pub one_day_price_change: Option<Decimal>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub adjudication: Option<Adjudication>,
    #[serde(default)]
    pub event_slug: Option<String>,
    #[serde(default)]
    pub neg_risk_augmented: Option<bool>,
    #[serde(default)]
    pub group_item_title: Option<String>,
    #[serde(default)]
    pub group_item_threshold: Option<i64>,
    #[serde(default)]
    pub sport_play_type: Option<String>,
    #[serde(default)]
    pub adapter_instance: Option<String>,
}

impl Market {
    /// Parse the `clob_token_ids` JSON-array-string into a `Vec<String>`.
    ///
    /// Returns an empty vec if the field is missing, empty, or unparseable.
    /// In a binary market `[0]` is the YES token id and `[1]` is the NO token id.
    #[must_use]
    pub fn parsed_clob_token_ids(&self) -> Vec<String> {
        let Some(raw) = self.clob_token_ids.as_deref() else {
            return Vec::new();
        };
        if raw.is_empty() {
            return Vec::new();
        }
        serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
    }
}

// ─── Events ─────────────────────────────────────────────────────────────────

/// A prediction-market event. Mirrors `gamma-service/internal/models.Event`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_translation: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub resolution_source: Option<String>,
    #[serde(default)]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub creation_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub closed: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub new: Option<bool>,
    #[serde(default)]
    pub featured: Option<bool>,
    #[serde(default)]
    pub restricted: Option<bool>,
    #[serde(default)]
    pub liquidity: Option<Decimal>,
    #[serde(default)]
    pub volume: Option<Decimal>,
    #[serde(default)]
    pub open_interest: Option<Decimal>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub volume24hr: Option<Decimal>,
    #[serde(default)]
    pub neg_risk: Option<bool>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub comment_count: Option<i64>,
    #[serde(default)]
    pub markets: Vec<Market>,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub num_markets: Option<i64>,
}

/// `Event` extended with per-tenant curation metadata (returned by `/curation/events`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurationEvent {
    #[serde(flatten)]
    pub event: Event,
    #[serde(default)]
    pub featured_level: i64,
    #[serde(default)]
    pub featured_order_normal: Option<i64>,
    #[serde(default)]
    pub featured_order_highlight: Option<i64>,
    #[serde(default)]
    pub featured_order_hero: Option<i64>,
}

// ─── Series ─────────────────────────────────────────────────────────────────

/// Recurring or tournament series grouping events. Mirrors `gamma-service/internal/models.Series`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub id: String,
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub series_type: Option<String>,
    #[serde(default)]
    pub recurrence: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub closed: Option<bool>,
    #[serde(default)]
    pub featured: Option<bool>,
    #[serde(default)]
    pub volume24hr: Option<Decimal>,
    #[serde(default)]
    pub volume: Option<Decimal>,
    #[serde(default)]
    pub liquidity: Option<Decimal>,
    #[serde(default)]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub comment_count: Option<i64>,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub chats: Vec<Chat>,
}

/// Lightweight series summary (`/series-summary/{id}` / `/series-summary/slug/{slug}`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SeriesSummary {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default, rename = "eventDates")]
    pub event_dates: Vec<String>,
    #[serde(default, rename = "eventWeeks")]
    pub event_weeks: Vec<i64>,
    #[serde(default)]
    pub earliest_open_week: Option<i64>,
    #[serde(default)]
    pub earliest_open_date: Option<String>,
}

// ─── Comments ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentPosition {
    #[serde(default)]
    pub token_id: Option<String>,
    #[serde(default)]
    pub position_size: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentProfile {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pseudonym: Option<String>,
    #[serde(default)]
    pub display_username_public: Option<bool>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub is_mod: Option<bool>,
    #[serde(default)]
    pub is_creator: Option<bool>,
    #[serde(default)]
    pub proxy_wallet: Option<String>,
    #[serde(default)]
    pub base_address: Option<String>,
    #[serde(default)]
    pub profile_image: Option<String>,
    #[serde(default)]
    pub profile_image_optimized: Option<ImageOptimization>,
    #[serde(default)]
    pub positions: Vec<CommentPosition>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub id: String,
    #[serde(default, rename = "commentID")]
    pub comment_id: Option<i64>,
    #[serde(default)]
    pub reaction_type: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub user_address: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub profile: Option<CommentProfile>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub parent_entity_type: Option<String>,
    #[serde(default, rename = "parentEntityID")]
    pub parent_entity_id: Option<i64>,
    #[serde(default, rename = "parentCommentID")]
    pub parent_comment_id: Option<String>,
    #[serde(default)]
    pub user_address: Option<String>,
    #[serde(default)]
    pub reply_address: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub profile: Option<CommentProfile>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub report_count: Option<i64>,
    #[serde(default)]
    pub reaction_count: Option<i64>,
}

/// Generic `{ "count": N }` envelope used by `/series/{id}/comments/count`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Count {
    pub count: i64,
}

// ─── Profiles ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PublicProfileUser {
    pub id: String,
    #[serde(default)]
    pub creator: bool,
    #[serde(default)]
    pub r#mod: bool,
}

/// `GET /public-profile` response.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProfile {
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub eoa_address: Option<String>,
    #[serde(default)]
    pub proxy_wallet: Option<String>,
    #[serde(default)]
    pub profile_image: Option<String>,
    #[serde(default)]
    pub display_username_public: Option<bool>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub pseudonym: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub users: Vec<PublicProfileUser>,
    #[serde(default)]
    pub x_username: Option<String>,
    #[serde(default)]
    pub verified_badge: Option<bool>,
}

/// `GET /profiles/user_address/{user_address}` response (full profile shape).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pseudonym: Option<String>,
    #[serde(default)]
    pub display_username_public: Option<bool>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub eoa_address: Option<String>,
    #[serde(default)]
    pub proxy_wallet: Option<String>,
    #[serde(default)]
    pub profile_image: Option<String>,
    #[serde(default)]
    pub profile_image_optimized: Option<ImageOptimization>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

// ─── Search ─────────────────────────────────────────────────────────────────

/// `GET /public-search` response (events + tags + profiles + pagination).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Search {
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub tags: Vec<SearchTag>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub pagination: Pagination,
}

// ─── Sports config ──────────────────────────────────────────────────────────

/// A single sport-type entry returned by `/config/sport-types`. The `label`
/// map is locale code -> display name (e.g. `{ "en": "Football", "zh": "足球" }`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SportType {
    pub value: String,
    #[serde(default)]
    pub label: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub stages: Vec<SportStage>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SportStage {
    pub value: String,
    #[serde(default)]
    pub label: std::collections::BTreeMap<String, String>,
}

/// Envelope `{ "types": [...] }` returned by `/config/sport-types`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SportTypesResponse {
    #[serde(default)]
    pub types: Vec<SportType>,
}

// ─── Public info / agreements ───────────────────────────────────────────────

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicInfoBrand {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub logo: String,
    #[serde(default)]
    pub title_translation: String,
    #[serde(default)]
    pub subtitle_translation: String,
    #[serde(default)]
    pub footer_config: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicInfoContracts {
    #[serde(default)]
    pub exchange_address: String,
    #[serde(default)]
    pub neg_risk_exchange_address: String,
    #[serde(default)]
    pub ctf_address: String,
    #[serde(default)]
    pub collateral_token: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicInfoApp {
    #[serde(default)]
    pub terms_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agreement {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub title_translation: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub content_translation: String,
    #[serde(default)]
    pub external_url: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub sort_order: i64,
}

/// `GET /public-info` response. `chain` is a raw JSON value because the
/// server returns either `{ chainId: N }` or a fully enriched chain object
/// depending on `kv_config` state.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicInfo {
    pub brand: PublicInfoBrand,
    #[serde(default)]
    pub chain: serde_json::Value,
    pub contracts: PublicInfoContracts,
    #[serde(default)]
    pub app: Option<PublicInfoApp>,
    #[serde(default)]
    pub login_statement: String,
    #[serde(default)]
    pub wallet_connect_project_id: String,
    #[serde(default)]
    pub agreements: Vec<Agreement>,
}

/// `GET /agreements` response envelope.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AgreementsResponse {
    #[serde(default)]
    pub agreements: Vec<Agreement>,
}

// ─── Health ─────────────────────────────────────────────────────────────────

/// `GET /health` response. The server includes a `service` discriminator,
/// `status`, and a millisecond timestamp.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub timestamp: i64,
}
