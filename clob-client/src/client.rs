//! Top-level [`Client`] for the chainup CLOB REST API.
//!
//! Phase 1 surface (public, no auth required):
//!
//! - [`Client::ok`] / [`Client::time`]
//! - [`Client::midpoint`] / [`Client::price`] / [`Client::spread`]
//! - [`Client::book`] (single-token order-book snapshot)
//! - [`Client::tick_size`] / [`Client::fee_rate`] / [`Client::last_trade_price`]
//!
//! Authenticated trading endpoints land in Phase 2 (see `pm-rs` README).

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client as HttpClient, Method};
use serde::de::DeserializeOwned;
use serde::Serialize;
use url::Url;

use crate::auth::Credentials;
use crate::clob::types::{
    FeeRateResponse, LastTradePriceResponse, MidpointResponse, OrderBookSummary, PriceResponse,
    SpreadResponse, TickSizeResponse,
};
use crate::error::{Error, Result};
use crate::types::Side;

const DEFAULT_USER_AGENT: &str = concat!("pm-rs-clob-client/", env!("CARGO_PKG_VERSION"));

/// Top-level CLOB client.
#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: HttpClient,
    base: Url,
    #[allow(dead_code)] // used by Phase 2 L2 paths
    credentials: Option<Credentials>,
}

impl Client {
    /// Construct a client pointed at the given REST endpoint.
    pub fn new(endpoint: impl AsRef<str>) -> Result<Self> {
        ClientBuilder::new().endpoint(endpoint).build()
    }

    /// Builder for additional knobs (timeout, user-agent, credentials).
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Health check — `GET /ok`. Returns the raw body (`"OK"` for the chainup server).
    pub async fn ok(&self) -> Result<String> {
        let url = self.endpoint("/ok")?;
        let resp = self.inner.http.get(url).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(Error::api(status, "GET", "/ok", body));
        }
        Ok(body)
    }

    /// Server time — `GET /time`. Returns a Unix timestamp (seconds).
    pub async fn time(&self) -> Result<i64> {
        // Chainup returns a bare integer; not JSON-object wrapped.
        let url = self.endpoint("/time")?;
        let resp = self.inner.http.get(url).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(Error::api(status, "GET", "/time", body));
        }
        body.trim()
            .parse::<i64>()
            .map_err(|e| Error::Validation(format!("/time returned non-integer body '{body}': {e}")))
    }

    /// Mid-price — `GET /midpoint?token_id=...`.
    pub async fn midpoint(&self, token_id: &str) -> Result<MidpointResponse> {
        self.get_json("/midpoint", &[("token_id", token_id)]).await
    }

    /// Best price for a side — `GET /price?token_id=...&side=buy|sell`.
    pub async fn price(&self, token_id: &str, side: Side) -> Result<PriceResponse> {
        let side_str = match side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        };
        self.get_json("/price", &[("token_id", token_id), ("side", side_str)])
            .await
    }

    /// Bid-ask spread — `GET /spread?token_id=...`.
    pub async fn spread(&self, token_id: &str) -> Result<SpreadResponse> {
        self.get_json("/spread", &[("token_id", token_id)]).await
    }

    /// Order book snapshot — `GET /book?token_id=...`.
    pub async fn book(&self, token_id: &str) -> Result<OrderBookSummary> {
        self.get_json("/book", &[("token_id", token_id)]).await
    }

    /// Tick size — `GET /tick-size?token_id=...`.
    pub async fn tick_size(&self, token_id: &str) -> Result<TickSizeResponse> {
        self.get_json("/tick-size", &[("token_id", token_id)]).await
    }

    /// Fee rate (bps) — `GET /fee-rate?token_id=...`.
    pub async fn fee_rate(&self, token_id: &str) -> Result<FeeRateResponse> {
        self.get_json("/fee-rate", &[("token_id", token_id)]).await
    }

    /// Last trade price — `GET /last-trade-price?token_id=...`.
    pub async fn last_trade_price(&self, token_id: &str) -> Result<LastTradePriceResponse> {
        self.get_json("/last-trade-price", &[("token_id", token_id)])
            .await
    }

    // ─── helpers ────────────────────────────────────────────────────────────

    fn endpoint(&self, path: &str) -> Result<Url> {
        // base URL is normalized (always trailing slash). Strip leading "/" from path so we join correctly.
        let p = path.trim_start_matches('/');
        Ok(self.inner.base.join(p)?)
    }

    async fn get_json<Q: Serialize, R: DeserializeOwned>(&self, path: &str, query: &Q) -> Result<R> {
        let url = self.endpoint(path)?;
        let resp = self.inner.http.get(url).query(query).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::api(status, "GET", path, body));
        }
        let value: serde_json::Value = resp.json().await?;
        let parsed = serde_json::from_value(value)
            .map_err(|e| Error::Validation(format!("decoding {path} response: {e}")))?;
        Ok(parsed)
    }

    // Methods used by Phase 2 (kept here as `pub(crate)` for visibility planning):

    #[allow(dead_code)]
    pub(crate) fn http(&self) -> &HttpClient {
        &self.inner.http
    }

    #[allow(dead_code)]
    pub(crate) fn base_url(&self) -> &Url {
        &self.inner.base
    }

    #[allow(dead_code)]
    pub(crate) fn credentials(&self) -> Option<&Credentials> {
        self.inner.credentials.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn require_credentials(&self) -> Result<&Credentials> {
        self.inner.credentials.as_ref().ok_or(Error::NotAuthenticated)
    }

    #[allow(dead_code)]
    pub(crate) async fn execute_raw(&self, method: Method, path: &str) -> Result<reqwest::Response> {
        let url = self.endpoint(path)?;
        let resp = self.inner.http.request(method, url).send().await?;
        Ok(resp)
    }
}

/// Builder for [`Client`].
#[derive(Debug)]
pub struct ClientBuilder {
    endpoint: String,
    timeout: Duration,
    user_agent: String,
    credentials: Option<Credentials>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: crate::DEFAULT_ENDPOINT.to_owned(),
            timeout: Duration::from_secs(30),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            credentials: None,
        }
    }

    #[must_use]
    pub fn endpoint(mut self, endpoint: impl AsRef<str>) -> Self {
        self.endpoint = endpoint.as_ref().to_owned();
        self
    }

    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    #[must_use]
    pub fn credentials(mut self, creds: Credentials) -> Self {
        self.credentials = Some(creds);
        self
    }

    pub fn build(self) -> Result<Client> {
        // Normalize endpoint to always end with "/"" so that join() works correctly.
        let mut endpoint = self.endpoint;
        if !endpoint.ends_with('/') {
            endpoint.push('/');
        }
        let base = Url::parse(&endpoint)?;
        let http = HttpClient::builder()
            .timeout(self.timeout)
            .user_agent(self.user_agent)
            .build()?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base,
                credentials: self.credentials,
            }),
        })
    }
}
