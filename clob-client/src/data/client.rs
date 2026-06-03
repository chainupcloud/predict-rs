//! HTTP client for the `data-service` (portfolio / trades / activity / leaderboards).
//!
//! Lives at `data-api.<tenant>`. Public read-only — no L1 / L2 auth required; tenant is
//! inferred from the HTTP `Host` header. Construct via [`crate::Client::data`]:
//!
//! ```no_run
//! use predict_rs_clob_client::Client;
//!
//! # async fn run() -> predict_rs_clob_client::Result<()> {
//! let client = Client::builder().tenant("hermestrade.xyz")?.build()?;
//! let data = client.data()?;
//! let positions = data.positions("0x7e63be993c5f51547609dedfa8f2398ebf7ac2fe", 25, None).await?;
//! # let _ = positions;
//! # Ok(())
//! # }
//! ```

use reqwest::{Client as HttpClient, Method};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::data::types::{
    Activity, ClosedPosition, HoldersBucket, LeaderboardResponse, LiveVolumeBucket,
    MarketPositionGroup, OpenInterestEntry, Position, PricesHistoryResponse, StatsResponse, Trade,
    TradedResponse, UnwrapRequest, UserPnlResponse,
};
use crate::error::{Error, Result};

/// Generic envelope used for several list endpoints (`positions`, `activity`,
/// `closed-positions`, `v1/market-positions`). The envelope is a divergence from
/// polymarket's data-api which returns flat arrays — so the SDK transparently unwraps it.
#[derive(serde::Deserialize)]
struct DataEnvelope<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

/// Sub-client for the `data-service` REST API. Constructed via [`crate::Client::data`].
///
/// Shares the underlying [`reqwest::Client`] with the parent CLOB client for connection
/// pooling.
#[derive(Clone, Debug)]
pub struct DataClient {
    http: HttpClient,
    base: Url,
}

impl DataClient {
    /// Construct directly. Most callers should use [`crate::Client::data`] instead.
    #[must_use]
    pub fn new(http: HttpClient, base: Url) -> Self {
        Self { http, base }
    }

    /// Base URL of this client (e.g. `https://data-api.hermestrade.xyz/`).
    #[must_use]
    pub fn base(&self) -> &Url {
        &self.base
    }

    // ─── /positions ────────────────────────────────────────────────────────

    /// `GET /positions` — open positions for a wallet (Safe / proxy wallet address).
    /// Server wraps the array in `{data: [...]}`; the SDK unwraps it transparently.
    pub async fn positions(
        &self,
        address: &str,
        limit: i32,
        offset: Option<i32>,
    ) -> Result<Vec<Position>> {
        let env: DataEnvelope<Position> = self
            .get_query("positions", &paginate(address, limit, offset))
            .await?;
        Ok(env.data)
    }

    /// `GET /closed-positions` — closed positions for a wallet.
    pub async fn closed_positions(
        &self,
        address: &str,
        limit: i32,
        offset: Option<i32>,
    ) -> Result<Vec<ClosedPosition>> {
        let env: DataEnvelope<ClosedPosition> = self
            .get_query("closed-positions", &paginate(address, limit, offset))
            .await?;
        Ok(env.data)
    }

    /// `GET /v1/market-positions` — leaderboard-style position list for one market,
    /// grouped by outcome token. Returns one group per token; each group lists the per-trader
    /// position rows. Wrapped in `{data: [...]}` (data envelope). Live wire uses
    /// `market=<conditionId>`, not `conditionId=`.
    pub async fn market_positions(
        &self,
        condition_id: &str,
        limit: i32,
        offset: Option<i32>,
    ) -> Result<Vec<MarketPositionGroup>> {
        let mut q = vec![
            ("market", condition_id.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(o) = offset {
            q.push(("offset", o.to_string()));
        }
        let env: DataEnvelope<MarketPositionGroup> =
            self.get_query("v1/market-positions", &q).await?;
        Ok(env.data)
    }

    // ─── /trades ───────────────────────────────────────────────────────────

    /// `GET /trades` — trade history for a wallet.
    pub async fn trades(
        &self,
        address: &str,
        limit: i32,
        offset: Option<i32>,
    ) -> Result<Vec<Trade>> {
        self.get_query("trades", &paginate(address, limit, offset)).await
    }

    // ─── /activity ─────────────────────────────────────────────────────────

    /// `GET /activity` — on-chain activity (trades + splits + merges + redeems + rewards) for a wallet.
    /// Wrapped in `{data: [...]}` (divergence from polymarket flat array).
    pub async fn activity(
        &self,
        address: &str,
        limit: i32,
        offset: Option<i32>,
    ) -> Result<Vec<Activity>> {
        let env: DataEnvelope<Activity> = self
            .get_query("activity", &paginate(address, limit, offset))
            .await?;
        Ok(env.data)
    }

    // ─── /holders ──────────────────────────────────────────────────────────

    /// `GET /holders` — top token holders for a market. Returns one bucket per token.
    pub async fn holders(
        &self,
        market: &str,
        limit: Option<i32>,
    ) -> Result<Vec<HoldersBucket>> {
        let mut q = vec![("market", market.to_string())];
        if let Some(l) = limit {
            q.push(("limit", l.to_string()));
        }
        self.get_query("holders", &q).await
    }

    // ─── /traded ───────────────────────────────────────────────────────────

    /// `GET /traded` — count of unique markets traded by a wallet.
    pub async fn traded(&self, address: &str) -> Result<TradedResponse> {
        self.get_query("traded", &[("user", address.to_string())]).await
    }

    // ─── /oi (open interest) ───────────────────────────────────────────────

    /// `GET /oi` — open interest for one market. Returns one entry per scope grouping
    /// (`condition` / `negRiskParent` / `sportsEvent`).
    pub async fn open_interest(&self, market: &str) -> Result<Vec<OpenInterestEntry>> {
        self.get_query("oi", &[("market", market.to_string())]).await
    }

    // ─── /live-volume ──────────────────────────────────────────────────────

    /// `GET /live-volume` — live volume for an event. Returns one bucket per parent (a
    /// neg-risk event yields one bucket; a sports event yields one per game).
    pub async fn live_volume(&self, id: &str) -> Result<Vec<LiveVolumeBucket>> {
        self.get_query("live-volume", &[("id", id.to_string())]).await
    }

    // ─── /prices-history ───────────────────────────────────────────────────

    /// `GET /prices-history` — token price history. `interval` accepts the
    /// granularity strings (`1m / 1h / 6h / 1d / max`). `fidelity` is the bucket
    /// resolution in seconds (defaults to interval-appropriate value).
    pub async fn prices_history(
        &self,
        market: &str,
        interval: Option<&str>,
        fidelity: Option<i64>,
    ) -> Result<PricesHistoryResponse> {
        let mut q = vec![("market", market.to_string())];
        if let Some(i) = interval {
            q.push(("interval", i.to_string()));
        }
        if let Some(f) = fidelity {
            q.push(("fidelity", f.to_string()));
        }
        self.get_query("prices-history", &q).await
    }

    // ─── /user-pnl ─────────────────────────────────────────────────────────

    /// `GET /user-pnl` — cumulative profit/loss time-series for a wallet.
    /// `interval` accepts `1d / 1w / 1m / all`. `fidelity` accepts `1h / 3h / 12h / 18h / 1d`.
    /// Live wire uses `user_address` as the address param (divergence from polymarket's `user`).
    pub async fn user_pnl(
        &self,
        address: &str,
        interval: Option<&str>,
        fidelity: Option<&str>,
    ) -> Result<UserPnlResponse> {
        let mut q = vec![("user_address", address.to_string())];
        if let Some(i) = interval {
            q.push(("interval", i.to_string()));
        }
        if let Some(f) = fidelity {
            q.push(("fidelity", f.to_string()));
        }
        self.get_query("user-pnl", &q).await
    }

    // ─── /stats ────────────────────────────────────────────────────────────

    /// `GET /stats` — global platform statistics.
    pub async fn stats(&self) -> Result<StatsResponse> {
        self.get_query::<[(&str, String); 0], _>("stats", &[]).await
    }

    // ─── /v1/leaderboard ──────────────────────────────────────────────────

    /// `GET /v1/leaderboard` — trader leaderboard with biggest-wins sidebar.
    /// `time_period` accepts `DAY / WEEK / MONTH / ALL` (case-insensitive on the server;
    /// default `DAY`). `order_by` accepts `PNL / VOL` (default `PNL`).
    pub async fn leaderboard(
        &self,
        time_period: Option<&str>,
        order_by: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<LeaderboardResponse> {
        let mut q = vec![];
        if let Some(tp) = time_period {
            q.push(("timePeriod", tp.to_string()));
        }
        if let Some(ob) = order_by {
            q.push(("orderBy", ob.to_string()));
        }
        if let Some(l) = limit {
            q.push(("limit", l.to_string()));
        }
        if let Some(o) = offset {
            q.push(("offset", o.to_string()));
        }
        self.get_query("v1/leaderboard", &q).await
    }

    // ─── /unwrap-requests ──────────────────────────────────────────────────

    /// `GET /unwrap-requests` — USDW unwrap queue for a Safe address.
    /// No Polymarket V1 equivalent.
    pub async fn unwrap_requests(
        &self,
        safe: &str,
        claimed: Option<bool>,
    ) -> Result<Vec<UnwrapRequest>> {
        let mut q = vec![("safe", safe.to_string())];
        if let Some(c) = claimed {
            q.push(("claimed", c.to_string()));
        }
        self.get_query("unwrap-requests", &q).await
    }

    // ─── HTTP helpers ──────────────────────────────────────────────────────

    fn url(&self, path: &str) -> Result<Url> {
        let p = path.trim_start_matches('/');
        Ok(self.base.join(p)?)
    }

    async fn get_query<Q, R>(&self, path: &str, query: &Q) -> Result<R>
    where
        Q: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self.url(path)?;
        let qs = serde_html_form::to_string(query)
            .map_err(|e| Error::Validation(format!("encode query for {path}: {e}")))?;
        let req = if qs.is_empty() {
            self.http.request(Method::GET, url)
        } else {
            let mut url = url;
            url.set_query(Some(&qs));
            self.http.request(Method::GET, url)
        };
        let resp = req.send().await?;
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

/// Build the standard `?user=<addr>&limit=N&offset=N` query tuple shared by /positions,
/// /closed-positions, /trades, /activity.
fn paginate(address: &str, limit: i32, offset: Option<i32>) -> Vec<(&'static str, String)> {
    let mut q = vec![
        ("user", address.to_string()),
        ("limit", limit.to_string()),
    ];
    if let Some(o) = offset {
        q.push(("offset", o.to_string()));
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{LeaderboardResponse, Position, StatsResponse, Trade};

    #[test]
    fn position_decodes_with_camelcase_fields_and_extra_fields() {
        let raw = r#"{
          "proxyWallet": "0x7e63",
          "asset": "1234",
          "conditionId": "0xcid",
          "size": 5.0, "avgPrice": 0.09,
          "initialValue": 0.45, "currentValue": 0.495,
          "cashPnl": 0.045, "percentPnl": 10.0,
          "totalBought": 0.45, "realizedPnl": 0.0,
          "percentRealizedPnl": 0.0, "curPrice": 0.099,
          "redeemable": false, "mergeable": true,
          "title": "Q?",
          "questionTranslation": "{\"zh_CN\":\"问?\"}",
          "eventTitleTranslation": "",
          "slug": "q", "icon": "i",
          "eventSlug": "e", "outcome": "Yes", "outcomeIndex": 0,
          "oppositeOutcome": "No", "oppositeAsset": "5678",
          "endDate": "2026-12-31", "negativeRisk": true,
          "eventType": "negrisk", "negRiskMarketId": "0xnr",
          "negRiskTotalOptions": 7
        }"#;
        let p: Position = serde_json::from_str(raw).unwrap();
        assert_eq!(p.proxy_wallet, "0x7e63");
        assert_eq!(p.size, 5.0);
        assert_eq!(p.question_translation, "{\"zh_CN\":\"问?\"}");
        assert!(p.negative_risk);
        assert_eq!(p.neg_risk_total_options, Some(7));
    }

    #[test]
    fn trade_decodes_with_fee_field() {
        let raw = r#"{
          "proxyWallet": "0x", "side": "BUY", "asset": "1",
          "conditionId": "0xc", "size": 5.0, "price": 0.09,
          "timestamp": 1779243592, "title": "Q",
          "slug": "q", "icon": "", "eventSlug": "e",
          "outcome": "Yes", "outcomeIndex": 0,
          "name": "", "pseudonym": "Quick Kudu", "bio": "",
          "profileImage": "", "profileImageOptimized": "",
          "transactionHash": "0x5657",
          "fee": 0.01
        }"#;
        let t: Trade = serde_json::from_str(raw).unwrap();
        assert_eq!(t.fee, 0.01);
        assert_eq!(t.pseudonym, "Quick Kudu");
    }

    #[test]
    fn leaderboard_decodes_wrapped_envelope() {
        let raw = r#"{
          "data": [{
            "rank": "1", "proxyWallet": "0x1", "userName": "alice",
            "profileImage": "", "xUsername": "", "verifiedBadge": false,
            "pnl": 1234.5, "vol": 9876.0
          }],
          "biggestWins": [{
            "username": "alice", "avatar": "", "address": "0x1",
            "title": "Q?", "slug": "q", "eventSlug": "e",
            "entryValue": 1.0, "exitValue": 50.0, "profit": 49.0
          }]
        }"#;
        let lb: LeaderboardResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(lb.data.len(), 1);
        assert_eq!(lb.biggest_wins.len(), 1);
        assert!(lb.errors.is_empty());
    }

    #[test]
    fn stats_decodes() {
        let raw = r#"{
          "totalVolume": 1.0, "volume24h": 0.5,
          "totalTrades": 100, "trades24h": 5,
          "activeMarkets": 7, "openInterest": 12.34
        }"#;
        let s: StatsResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.total_volume, 1.0);
        assert_eq!(s.active_markets, 7);
    }

    #[test]
    fn paginate_includes_offset_only_when_provided() {
        let q = paginate("0xabc", 25, Some(50));
        assert_eq!(q.len(), 3);
        assert_eq!(q[2], ("offset", "50".to_string()));
        let q2 = paginate("0xabc", 25, None);
        assert_eq!(q2.len(), 2);
    }
}
