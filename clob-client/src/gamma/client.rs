//! HTTP client for the chainup Gamma API (market metadata).
//!
//! Gamma is a separate REST service from CLOB; in chainup it lives at
//! `gamma-api.<tenant>` (e.g. `https://gamma-api.hermestrade.xyz`). It provides
//! events, markets, tags, series, comments, profiles, search, and per-tenant
//! curation / public-info endpoints.
//!
//! Construct a `GammaClient` via [`crate::Client::gamma`]:
//!
//! ```no_run
//! use pm_rs_clob_client::{Client, gamma::types::request::ListEventsRequest};
//!
//! # async fn run() -> pm_rs_clob_client::Result<()> {
//! let client = Client::builder().tenant("hermestrade.xyz")?.build()?;
//! let gamma = client.gamma()?;
//! let events = gamma.list_events(&ListEventsRequest { limit: Some(5), ..Default::default() }).await?;
//! # let _ = events;
//! # Ok(())
//! # }
//! ```

use reqwest::{Client as HttpClient, Method};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{Error, Result};
use crate::gamma::types::request::{
    CommentsByUserAddressRequest, GetSeriesRequest, ListCommentsRequest, ListCurationEventsRequest,
    ListEventCreatorsRequest, ListEventsRequest, ListSeriesRequest, ListTagsRequest,
    RelatedTagsRequest, SearchRequest,
};
use crate::gamma::types::response::{
    Agreement, AgreementsResponse, Comment, Count, CurationEvent, Event, EventCreator,
    HealthResponse, Market, Profile, PublicInfo, PublicProfile, RelatedTag, Search, Series,
    SeriesSummary, SportTypesResponse, Tag,
};

/// Sub-client for the Gamma REST API. Constructed via [`crate::Client::gamma`].
///
/// Shares the underlying [`reqwest::Client`] with the parent CLOB client so
/// HTTP connection pooling is preserved.
#[derive(Clone, Debug)]
pub struct GammaClient {
    http: HttpClient,
    base: Url,
}

impl GammaClient {
    /// Construct directly. Most callers should use [`crate::Client::gamma`] instead.
    #[must_use]
    pub fn new(http: HttpClient, base: Url) -> Self {
        Self { http, base }
    }

    /// Base URL of this client (e.g. `https://gamma-api.hermestrade.xyz/`).
    #[must_use]
    pub fn base(&self) -> &Url {
        &self.base
    }

    // ─── System ─────────────────────────────────────────────────────────────

    /// `GET /health` — service liveness probe.
    pub async fn health(&self) -> Result<HealthResponse> {
        self.get_json("health", &()).await
    }

    /// `GET /public-info` — tenant brand and chain configuration.
    pub async fn public_info(&self) -> Result<PublicInfo> {
        self.get_json("public-info", &()).await
    }

    /// `GET /agreements` — enabled agreements for the current tenant.
    pub async fn agreements(&self) -> Result<Vec<Agreement>> {
        let envelope: AgreementsResponse = self.get_json("agreements", &()).await?;
        Ok(envelope.agreements)
    }

    /// `GET /config/sport-types` — platform-level sport-type catalog with stages.
    pub async fn sport_types(&self) -> Result<SportTypesResponse> {
        self.get_json("config/sport-types", &()).await
    }

    // ─── Tags ───────────────────────────────────────────────────────────────

    /// `GET /tags` — tenant-scoped tag list.
    pub async fn list_tags(&self, req: &ListTagsRequest) -> Result<Vec<Tag>> {
        self.get_json("tags", req).await
    }

    /// `GET /tags/{id}` — tag by numeric ID.
    pub async fn get_tag(&self, id: &str) -> Result<Tag> {
        self.get_json(&format!("tags/{id}"), &()).await
    }

    /// `GET /tags/slug/{slug}` — tag by slug.
    pub async fn get_tag_by_slug(&self, slug: &str) -> Result<Tag> {
        self.get_json(&format!("tags/slug/{slug}"), &()).await
    }

    /// `GET /tags/{id}/related-tags` — related-tag relationship rows by tag ID.
    pub async fn related_tags(&self, id: &str, req: &RelatedTagsRequest) -> Result<Vec<RelatedTag>> {
        self.get_json(&format!("tags/{id}/related-tags"), req).await
    }

    /// `GET /tags/slug/{slug}/related-tags` — related-tag rows by tag slug.
    pub async fn related_tags_by_slug(
        &self,
        slug: &str,
        req: &RelatedTagsRequest,
    ) -> Result<Vec<RelatedTag>> {
        self.get_json(&format!("tags/slug/{slug}/related-tags"), req)
            .await
    }

    /// `GET /tags/{id}/related-tags/tags` — full `Tag` objects related to a tag ID.
    pub async fn tags_related_to_tag(
        &self,
        id: &str,
        req: &RelatedTagsRequest,
    ) -> Result<Vec<Tag>> {
        self.get_json(&format!("tags/{id}/related-tags/tags"), req)
            .await
    }

    /// `GET /tags/slug/{slug}/related-tags/tags` — full `Tag` objects related to a tag slug.
    pub async fn tags_related_to_tag_by_slug(
        &self,
        slug: &str,
        req: &RelatedTagsRequest,
    ) -> Result<Vec<Tag>> {
        self.get_json(&format!("tags/slug/{slug}/related-tags/tags"), req)
            .await
    }

    // ─── Events ─────────────────────────────────────────────────────────────

    /// `GET /events` — tenant-scoped event list with filtering and pagination.
    pub async fn list_events(&self, req: &ListEventsRequest) -> Result<Vec<Event>> {
        self.get_json("events", req).await
    }

    /// `GET /events/{id}` — event by numeric ID.
    pub async fn get_event(&self, id: &str) -> Result<Event> {
        self.get_json(&format!("events/{id}"), &()).await
    }

    /// `GET /events/slug/{slug}` — event by slug.
    pub async fn get_event_by_slug(&self, slug: &str) -> Result<Event> {
        self.get_json(&format!("events/slug/{slug}"), &()).await
    }

    /// `GET /events/{id}/tags` — tags attached to an event.
    pub async fn event_tags(&self, id: &str) -> Result<Vec<Tag>> {
        self.get_json(&format!("events/{id}/tags"), &()).await
    }

    /// `GET /events/creators` — event creator list.
    pub async fn list_event_creators(
        &self,
        req: &ListEventCreatorsRequest,
    ) -> Result<Vec<EventCreator>> {
        self.get_json("events/creators", req).await
    }

    /// `GET /events/creators/{id}` — event creator by ID.
    pub async fn get_event_creator(&self, id: &str) -> Result<EventCreator> {
        self.get_json(&format!("events/creators/{id}"), &()).await
    }

    /// `GET /curation/events` — per-tenant featured / hero / highlighted events.
    pub async fn list_curation_events(
        &self,
        req: &ListCurationEventsRequest,
    ) -> Result<Vec<CurationEvent>> {
        self.get_json("curation/events", req).await
    }

    // ─── Markets ────────────────────────────────────────────────────────────

    /// `GET /markets/{id}` — market by numeric ID.
    ///
    /// Pass `include_tag = true` to embed the market's tags in the response.
    pub async fn get_market(&self, id: &str, include_tag: bool) -> Result<Market> {
        let path = format!("markets/{id}");
        if include_tag {
            self.get_json(&path, &[("include_tag", "true")]).await
        } else {
            self.get_json(&path, &()).await
        }
    }

    /// `GET /markets/slug/{slug}` — market by slug.
    pub async fn get_market_by_slug(&self, slug: &str, include_tag: bool) -> Result<Market> {
        let path = format!("markets/slug/{slug}");
        if include_tag {
            self.get_json(&path, &[("include_tag", "true")]).await
        } else {
            self.get_json(&path, &()).await
        }
    }

    /// `GET /markets/{id}/tags` — tags attached to a market.
    pub async fn market_tags(&self, id: &str) -> Result<Vec<Tag>> {
        self.get_json(&format!("markets/{id}/tags"), &()).await
    }

    /// `POST /markets/information` — bulk-fetch markets by id / slug / clob token /
    /// condition id / date range. Body is a free-form JSON object; common keys
    /// match `gamma-service` `MarketsInformationBody` (`id`, `slug`,
    /// `clobTokenIds`, `conditionIds`, `marketMakerAddress`, `closed`,
    /// `liquidityNumMin`, `liquidityNumMax`, `volumeNumMin`, `volumeNumMax`,
    /// `startDateMin/Max`, `endDateMin/Max`, `tagId`, `relatedTags`, `cyom`,
    /// `umaResolutionStatus`, `gameId`, `sportsMarketTypes`, `rewardsMinSize`,
    /// `questionIds`, `includeTags`).
    pub async fn markets_information(
        &self,
        body: &serde_json::Value,
    ) -> Result<Vec<Market>> {
        let url = self.url("markets/information")?;
        let resp = self.http.post(url).json(body).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::api(
                status,
                "POST",
                "/markets/information",
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| Error::Validation(format!("decoding /markets/information: {e}")))
    }

    // ─── Series ─────────────────────────────────────────────────────────────

    /// `GET /series` — series list.
    pub async fn list_series(&self, req: &ListSeriesRequest) -> Result<Vec<Series>> {
        self.get_json("series", req).await
    }

    /// `GET /series/{id}` — series by ID. The `req` flags control whether nested
    /// events and chat / comment counts are embedded.
    pub async fn get_series(&self, id: &str, req: &GetSeriesRequest) -> Result<Series> {
        self.get_json(&format!("series/{id}"), req).await
    }

    /// `GET /series/{id}/comments/count` — comment-count envelope for a series.
    pub async fn series_comment_count(&self, id: &str) -> Result<Count> {
        self.get_json(&format!("series/{id}/comments/count"), &())
            .await
    }

    /// `GET /series-summary/{id}` — slim series summary by ID.
    pub async fn get_series_summary(&self, id: &str) -> Result<SeriesSummary> {
        self.get_json(&format!("series-summary/{id}"), &()).await
    }

    /// `GET /series-summary/slug/{slug}` — slim series summary by slug.
    pub async fn get_series_summary_by_slug(&self, slug: &str) -> Result<SeriesSummary> {
        self.get_json(&format!("series-summary/slug/{slug}"), &())
            .await
    }

    // ─── Comments ───────────────────────────────────────────────────────────

    /// `GET /comments` — filtered comment list.
    pub async fn list_comments(&self, req: &ListCommentsRequest) -> Result<Vec<Comment>> {
        self.get_json("comments", req).await
    }

    /// `GET /comments/{id}` — comment thread by ID (includes nested replies).
    pub async fn get_comment(&self, id: &str) -> Result<Vec<Comment>> {
        self.get_json(&format!("comments/{id}"), &()).await
    }

    /// `GET /comments/user_address/{user_address}` — comments authored by a wallet.
    pub async fn comments_by_user(
        &self,
        address: &str,
        req: &CommentsByUserAddressRequest,
    ) -> Result<Vec<Comment>> {
        self.get_json(&format!("comments/user_address/{address}"), req)
            .await
    }

    // ─── Profiles ───────────────────────────────────────────────────────────

    /// `GET /public-profile` — slim public profile by wallet address.
    pub async fn get_public_profile(&self, address: &str) -> Result<PublicProfile> {
        self.get_json("public-profile", &[("address", address)])
            .await
    }

    /// `GET /profiles/user_address/{user_address}` — full profile by wallet address.
    pub async fn get_profile_by_address(&self, address: &str) -> Result<Profile> {
        self.get_json(&format!("profiles/user_address/{address}"), &())
            .await
    }

    // ─── Search ─────────────────────────────────────────────────────────────

    /// `GET /public-search` — text search across events, tags, and profiles.
    pub async fn search(&self, req: &SearchRequest) -> Result<Search> {
        self.get_json("public-search", req).await
    }

    // ─── HTTP helpers ───────────────────────────────────────────────────────

    fn url(&self, path: &str) -> Result<Url> {
        let p = path.trim_start_matches('/');
        Ok(self.base.join(p)?)
    }

    async fn get_json<Q, R>(&self, path: &str, query: &Q) -> Result<R>
    where
        Q: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self.url(path)?;
        // serde_html_form gives us repeated keys for Vec<T> and skip-None for Option<T>,
        // which `reqwest::RequestBuilder::query` (backed by serde_urlencoded) does not.
        let qs = serde_html_form::to_string(query)
            .map_err(|e| Error::Validation(format!("encode query for {path}: {e}")))?;
        let resp = if qs.is_empty() {
            self.http.request(Method::GET, url).send().await?
        } else {
            // Append manually so we can deliver the serde_html_form-encoded form.
            let mut url = url;
            url.set_query(Some(&qs));
            self.http.request(Method::GET, url).send().await?
        };
        let status = resp.status();
        let bytes = resp.bytes().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::api(
                status,
                "GET",
                path,
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| Error::Validation(format!("decoding {path}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use crate::gamma::types::request::{ListEventsRequest, ListTagsRequest, SearchRequest};

    #[test]
    fn events_query_serializes_with_camelcase_keys() {
        // The chainup server reads `tag_id`, `start_date_min` etc. as snake_case.
        let req = ListEventsRequest {
            limit: Some(20),
            offset: Some(40),
            order: Some("created_at".into()),
            ascending: Some(true),
            tag_id: Some(42),
            active: Some(true),
            ..Default::default()
        };
        let qs = serde_html_form::to_string(&req).unwrap();
        assert!(qs.contains("limit=20"));
        assert!(qs.contains("offset=40"));
        assert!(qs.contains("order=created_at"));
        assert!(qs.contains("ascending=true"));
        assert!(qs.contains("tag_id=42"));
        assert!(qs.contains("active=true"));
        // None-valued fields must not appear.
        assert!(!qs.contains("closed"));
        assert!(!qs.contains("featured"));
    }

    #[test]
    fn empty_request_serializes_to_empty_string() {
        let qs = serde_html_form::to_string(ListTagsRequest::default()).unwrap();
        assert_eq!(qs, "");
    }

    #[test]
    fn search_request_serializes_arrays_as_repeated_keys() {
        let req = SearchRequest {
            q: "world cup".into(),
            limit_per_type: Some(5),
            events_tag: vec!["football".into(), "uefa".into()],
            exclude_tag_id: vec![1, 2],
            ..Default::default()
        };
        let qs = serde_html_form::to_string(&req).unwrap();
        assert!(qs.contains("q=world+cup") || qs.contains("q=world%20cup"));
        assert!(qs.contains("limit_per_type=5"));
        assert!(qs.contains("events_tag=football"));
        assert!(qs.contains("events_tag=uefa"));
        assert!(qs.contains("exclude_tag_id=1"));
        assert!(qs.contains("exclude_tag_id=2"));
    }
}
