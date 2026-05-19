//! Query-parameter request structs for the chainup Gamma API.
//!
//! Each struct serialises directly into a URL query string via `serde_html_form`.
//! Optional fields wrapped in `Option<T>` are skipped when `None`, matching
//! upstream server behaviour (omit -> default). Pagination defaults (`limit=20`,
//! `offset=0`) live on the server side, so we send nothing unless the caller
//! sets a value.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// `GET /events` query parameters. Mirrors `gamma-service` `ListEvents` handler.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ListEventsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Sort field — one of: `id`, `label`, `slug`, `created_at`, `updated_at`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date_min: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date_max: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date_min: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date_max: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_min: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_max: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_max: Option<f64>,
}

/// `GET /events/creators` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ListEventCreatorsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_handle: Option<String>,
}

/// `GET /tags` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ListTagsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    /// When `true`, returns only tags marked for navigation-bar display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_carousel: Option<bool>,
}

/// `GET /tags/{id}/related-tags` and `GET /tags/slug/{slug}/related-tags` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RelatedTagsRequest {
    /// Filter related rows by `predict_tags.status` (e.g. `"active"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// When `true`, omits relationships whose related-tag row is empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit_empty: Option<bool>,
}

/// `GET /series` query parameters. `slug` and the category arrays repeat the
/// same key (`?slug=a&slug=b`) per the chi handler implementation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ListSeriesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slug: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_events: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories_labels: Vec<String>,
}

/// `GET /series/{id}` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct GetSeriesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_events: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_chat: Option<bool>,
}

/// `GET /comments` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ListCommentsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    /// Entity type — `"Event"`, `"Market"`, or `"Series"` (case-sensitive on the server).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_entity_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_entity_id: Option<i64>,
}

/// `GET /comments/user_address/{user_address}` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CommentsByUserAddressRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
}

/// `GET /public-search` query parameters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SearchRequest {
    pub q: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_per_type: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
    /// Default: search tags too. Set to `false` to skip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_tags: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_profiles: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events_tag: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_tag_id: Vec<i64>,
}

/// `GET /curation/events` query parameters. `featured_level` is a bitmask
/// filter — `0` (or omitted) returns all featured events.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ListCurationEventsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured_level: Option<i64>,
}
