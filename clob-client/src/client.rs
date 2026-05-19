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
//!
//! Multi-endpoint configuration mirrors `pm-sdk-go`'s `WithEndpoints(clob, gamma, ws)`:
//!
//! ```no_run
//! use pm_rs_clob_client::{Client, Endpoints};
//!
//! # async fn run() -> pm_rs_clob_client::Result<()> {
//! // Explicit three-URL form (matches pm-sdk-go.WithEndpoints):
//! let client = Client::builder()
//!     .endpoints(Endpoints::new(
//!         "https://clob-api.hermestrade.xyz",
//!         "https://gamma-api.hermestrade.xyz",
//!         "wss://clob-ws.hermestrade.xyz",
//!     )?)
//!     .chain_id(143)
//!     .user_agent("my-app/1.0")
//!     .build()?;
//!
//! // Or derive from a tenant host using the canonical subdomain pattern:
//! let client = Client::builder()
//!     .tenant("hermestrade.xyz")?
//!     .chain_id(143)
//!     .build()?;
//!
//! let _time = client.time().await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client as HttpClient, Method};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::auth::Credentials;
use crate::clob::types::{
    FeeRateResponse, LastTradePriceResponse, MidpointResponse, OrderBookSummary, PriceResponse,
    SpreadResponse, TickSizeResponse,
};
use crate::endpoints::Endpoints;
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
    endpoints: Endpoints,
    chain_id: Option<u64>,
    #[allow(dead_code)] // used by Phase 2 L2 paths
    credentials: Option<Credentials>,
}

impl Client {
    /// Convenience: build a client with only a CLOB endpoint set (Phase 1 read-only).
    pub fn new(clob_endpoint: impl AsRef<str>) -> Result<Self> {
        ClientBuilder::new().clob_endpoint(clob_endpoint).build()
    }

    /// Builder for full configuration (multi-endpoint, chain id, timeout, credentials).
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// CLOB base URL.
    #[must_use]
    pub fn clob_url(&self) -> &Url {
        &self.inner.endpoints.clob
    }

    /// Gamma base URL (Phase 3).
    #[must_use]
    pub fn gamma_url(&self) -> Option<&Url> {
        self.inner.endpoints.gamma.as_ref()
    }

    /// WebSocket base URL (Phase 3).
    #[must_use]
    pub fn ws_url(&self) -> Option<&Url> {
        self.inner.endpoints.ws.as_ref()
    }

    /// Configured chain id (None if the caller did not set one — Phase 1 read-only flows don't need it).
    #[must_use]
    pub fn chain_id(&self) -> Option<u64> {
        self.inner.chain_id
    }

    /// Health check — `GET /ok`. Returns the raw body (`"OK"` for the chainup server).
    pub async fn ok(&self) -> Result<String> {
        let url = self.clob("/ok")?;
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
        let url = self.clob("/time")?;
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

    fn clob(&self, path: &str) -> Result<Url> {
        let p = path.trim_start_matches('/');
        Ok(self.inner.endpoints.clob.join(p)?)
    }

    async fn get_json<Q: Serialize, R: DeserializeOwned>(&self, path: &str, query: &Q) -> Result<R> {
        let url = self.clob(path)?;
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
    pub(crate) fn credentials(&self) -> Option<&Credentials> {
        self.inner.credentials.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn require_credentials(&self) -> Result<&Credentials> {
        self.inner.credentials.as_ref().ok_or(Error::NotAuthenticated)
    }

    #[allow(dead_code)]
    pub(crate) async fn execute_raw(&self, method: Method, path: &str) -> Result<reqwest::Response> {
        let url = self.clob(path)?;
        let resp = self.inner.http.request(method, url).send().await?;
        Ok(resp)
    }
}

/// Builder for [`Client`].
#[derive(Debug)]
pub struct ClientBuilder {
    endpoints: Option<Endpoints>,
    chain_id: Option<u64>,
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
            endpoints: None,
            chain_id: None,
            timeout: Duration::from_secs(30),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            credentials: None,
        }
    }

    /// Supply the three service endpoints explicitly. Mirrors `pm-sdk-go.WithEndpoints(clob, gamma, ws)`.
    #[must_use]
    pub fn endpoints(mut self, endpoints: Endpoints) -> Self {
        self.endpoints = Some(endpoints);
        self
    }

    /// Derive all three endpoints from a tenant root host using the canonical chainup subdomain
    /// pattern (`clob-api.<host>` / `gamma-api.<host>` / `clob-ws.<host>`).
    pub fn tenant(mut self, tenant_host: impl AsRef<str>) -> Result<Self> {
        self.endpoints = Some(Endpoints::from_tenant(tenant_host)?);
        Ok(self)
    }

    /// Set the CLOB REST URL only (Gamma / WS unset). Phase 1 convenience.
    #[must_use]
    pub fn clob_endpoint(mut self, clob: impl AsRef<str>) -> Self {
        // Preserve any previously-set gamma/ws so callers can chain individual setters.
        match self.endpoints {
            Some(ref mut ep) => match Endpoints::clob_only(clob) {
                Ok(new) => ep.clob = new.clob,
                Err(_) => {
                    self.endpoints = None;
                }
            },
            None => {
                self.endpoints = Endpoints::clob_only(clob).ok();
            }
        }
        self
    }

    /// Set the Gamma REST URL (Phase 3).
    #[must_use]
    pub fn gamma_endpoint(mut self, gamma: impl AsRef<str>) -> Self {
        if let Some(ref mut ep) = self.endpoints {
            if let Ok(parsed) = parse_url(gamma.as_ref()) {
                ep.gamma = Some(parsed);
            }
        }
        self
    }

    /// Set the WebSocket URL (Phase 3).
    #[must_use]
    pub fn ws_endpoint(mut self, ws: impl AsRef<str>) -> Self {
        if let Some(ref mut ep) = self.endpoints {
            if let Ok(parsed) = parse_url(ws.as_ref()) {
                ep.ws = Some(parsed);
            }
        }
        self
    }

    /// Configure chain id. Required for signing flows (Phase 2+); Phase 1 read-only paths don't need it.
    #[must_use]
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = Some(chain_id);
        self
    }

    /// HTTP request timeout. Default: 30 s.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// HTTP `User-Agent` header.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    /// L2 credentials (Phase 2).
    #[must_use]
    pub fn credentials(mut self, creds: Credentials) -> Self {
        self.credentials = Some(creds);
        self
    }

    pub fn build(self) -> Result<Client> {
        let endpoints = self
            .endpoints
            .ok_or_else(|| Error::validation("no endpoints configured: call .endpoints() / .tenant() / .clob_endpoint() before .build()"))?;
        let http = HttpClient::builder()
            .timeout(self.timeout)
            .user_agent(self.user_agent)
            .build()?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                endpoints,
                chain_id: self.chain_id,
                credentials: self.credentials,
            }),
        })
    }
}

fn parse_url(s: &str) -> Result<Url> {
    let mut s = s.to_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    Ok(Url::parse(&s)?)
}
