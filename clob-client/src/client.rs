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

use reqwest::header::HeaderMap;
use reqwest::{Client as HttpClient, Method, RequestBuilder};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;
use uuid::Uuid;

use crate::auth::{
    Credentials, build_l1_headers, build_l2_headers, current_timestamp,
};
use crate::clob::order_builder::{Limit, Market, OrderBuilder};
use crate::clob::types::{
    ApiKeyInfo, AssetType, BalanceAllowanceResponse, CancelMarketOrderRequest,
    CancelOrdersResponse, FeeRateResponse, HeartbeatResponse, LastTradePriceResponse,
    MidpointResponse, OpenOrderResponse, OrderBookSummary, OrderScoringResponse, OrdersRequest,
    Page, PostOrderResponse, PriceResponse, ReplaceOrdersRequest, ReplaceOrdersResponse,
    SendOrderRequest, SignedOrder, SpreadResponse, TickSizeResponse, TradeResponse,
    TradesRequest,
};
use crate::endpoints::Endpoints;
use crate::error::{Error, Result};
use crate::signer::PMCup26Signer;
use crate::types::Side;
use std::collections::HashMap;

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
    credentials: Option<Credentials>,
    /// EOA address of the configured L1 signer. Required for L2 calls (`PRED_ADDRESS`
    /// header); optional when only public market-data endpoints are used.
    signer_address: Option<crate::types::Address>,
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

    /// Construct a [`crate::gamma::GammaClient`] sharing this client's HTTP pool.
    ///
    /// Errors with [`Error::Validation`] if no Gamma endpoint was configured
    /// (i.e. the client was built via `--clob-endpoint` only, without `--tenant`
    /// or an explicit `--gamma-endpoint`).
    pub fn gamma(&self) -> Result<crate::gamma::GammaClient> {
        let base = self.gamma_url().ok_or_else(|| {
            Error::validation(
                "gamma endpoint not configured: pass --gamma-endpoint or use --tenant",
            )
        })?;
        Ok(crate::gamma::GammaClient::new(
            self.inner.http.clone(),
            base.clone(),
        ))
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

    // ─── Phase 2.1: L1 auth — API key CRUD ──────────────────────────────

    /// Idempotent: try `POST /auth/api-key` first; on any 4xx response fall back to
    /// `GET /auth/derive-api-key`. Mirrors `rs-clob-client`'s
    /// `Client::create_or_derive_api_key` flow with chainup `PRED_*` headers.
    pub async fn create_or_derive_api_key(
        &self,
        signer: &PMCup26Signer,
        nonce: Option<u32>,
    ) -> Result<Credentials> {
        match self.create_api_key(signer, nonce).await {
            Ok(creds) => Ok(creds),
            // Server responded with HTTP error (e.g. 409 duplicate / 400 invalid request).
            // Network / decoding failures bubble up unchanged.
            Err(Error::Api { .. }) => self.derive_api_key(signer, nonce).await,
            Err(other) => Err(other),
        }
    }

    /// `POST /auth/api-key` — create a new L2 API key bound to `(signer.address, scope_id, nonce)`.
    pub async fn create_api_key(
        &self,
        signer: &PMCup26Signer,
        nonce: Option<u32>,
    ) -> Result<Credentials> {
        let headers = build_l1_headers(signer, nonce)?;
        let resp = self
            .send_with_headers(Method::POST, "/auth/api-key", None, headers, None)
            .await?;
        let body = check_ok(resp, "POST", "/auth/api-key").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /auth/api-key response: {e}")))
    }

    /// `GET /auth/derive-api-key` — recover the credentials for an existing key without
    /// minting a new one. Signs the same `ClobAuth` payload as [`Self::create_api_key`].
    pub async fn derive_api_key(
        &self,
        signer: &PMCup26Signer,
        nonce: Option<u32>,
    ) -> Result<Credentials> {
        let headers = build_l1_headers(signer, nonce)?;
        let resp = self
            .send_with_headers(Method::GET, "/auth/derive-api-key", None, headers, None)
            .await?;
        let body = check_ok(resp, "GET", "/auth/derive-api-key").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /auth/derive-api-key response: {e}")))
    }

    /// `DELETE /auth/api-key` — revoke the L2 key for `(signer.address, scope_id, nonce)`.
    ///
    /// The `key` argument is accepted for API symmetry with the rs-clob-client method but is
    /// **not** sent — the server identifies the row by `(address, scope, nonce)` and ignores
    /// any body. Pass [`Uuid::nil`] if you only have `(signer, nonce)` in hand.
    pub async fn delete_api_key(&self, signer: &PMCup26Signer, key: Uuid) -> Result<()> {
        let _ = key;
        // DELETE uses the same L1 headers as POST / GET (nonce defaults to 0).
        let headers = build_l1_headers(signer, None)?;
        let resp = self
            .send_with_headers(Method::DELETE, "/auth/api-key", None, headers, None)
            .await?;
        check_ok(resp, "DELETE", "/auth/api-key").await?;
        Ok(())
    }

    // ─── Phase 2.1: L2 auth — read methods ──────────────────────────────

    /// `GET /auth/api-keys` — list active API keys + chainup `proxy_wallet` for the
    /// authenticated address. Requires [`ClientBuilder::credentials`] + [`ClientBuilder::chain_id`].
    pub async fn api_keys(&self) -> Result<ApiKeyInfo> {
        self.l2_get_json::<ApiKeyInfo>("/auth/api-keys", &[]).await
    }

    /// `GET /balance-allowance?asset_type=...&token_id=...` — Safe-wallet balance + allowances
    /// for the authenticated address. The server derives the Safe address from `EOA + scopeId`.
    ///
    /// Validation matches the server:
    /// - `Conditional` requires `token_id`.
    /// - `Collateral` must NOT carry a `token_id`.
    pub async fn balance_allowance(
        &self,
        asset_type: AssetType,
        token_id: Option<&str>,
    ) -> Result<BalanceAllowanceResponse> {
        let query = balance_allowance_query(asset_type, token_id)?;
        self.l2_get_json::<BalanceAllowanceResponse>("/balance-allowance", &query)
            .await
    }

    /// `GET /balance-allowance/update?asset_type=...&token_id=...` — force the server to refresh
    /// its subgraph balance cache, then return the same shape as [`Self::balance_allowance`].
    pub async fn update_balance_allowance(
        &self,
        asset_type: AssetType,
        token_id: Option<&str>,
    ) -> Result<BalanceAllowanceResponse> {
        let query = balance_allowance_query(asset_type, token_id)?;
        self.l2_get_json::<BalanceAllowanceResponse>("/balance-allowance/update", &query)
            .await
    }

    // ─── L2 HTTP plumbing ───────────────────────────────────────────────

    /// Issue an L2-authenticated request with optional query and JSON body. Constructs the
    /// HMAC over the **path only** (no query string) per the server's
    /// `middleware/auth.go::computeHMAC` contract, then attaches the five `PRED_*` headers.
    pub(crate) async fn request_authenticated(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<reqwest::Response> {
        let creds = self.require_credentials()?;
        let address = self.require_signer_address()?;
        let timestamp = current_timestamp();
        let method_str = method.as_str().to_owned();
        let body_str = body.unwrap_or("");
        let headers = build_l2_headers(creds, address, &timestamp, &method_str, path, body_str)?;
        self.send_with_headers(method, path, Some(query), headers, body.map(str::to_owned))
            .await
    }

    /// L2 GET → JSON helper.
    async fn l2_get_json<R: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<R> {
        let resp = self
            .request_authenticated(Method::GET, path, query, None)
            .await?;
        let body = check_ok(resp, "GET", path).await?;
        serde_json::from_str::<R>(&body)
            .map_err(|e| Error::Validation(format!("decoding {path} response: {e}")))
    }

    async fn send_with_headers(
        &self,
        method: Method,
        path: &str,
        query: Option<&[(&str, &str)]>,
        headers: HeaderMap,
        body: Option<String>,
    ) -> Result<reqwest::Response> {
        let url = self.clob(path)?;
        let mut req: RequestBuilder = self.inner.http.request(method, url).headers(headers);
        if let Some(q) = query {
            if !q.is_empty() {
                req = req.query(q);
            }
        }
        if let Some(b) = body {
            req = req.header(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("application/json"),
            );
            req = req.body(b);
        }
        Ok(req.send().await?)
    }

    // ─── Phase 2.2: order / trade endpoints ─────────────────────────────

    /// Begin building a limit order. Returns an [`OrderBuilder`] in the [`Limit`] state.
    ///
    /// Pre-populating `feeRateBps` + `minimum_tick_size` from `GET /fee-rate` and
    /// `GET /tick-size` is **not** done here to keep this synchronous; callers that want
    /// auto-discovery should:
    ///
    /// ```ignore
    /// let fee = client.fee_rate(token).await?;
    /// let tick = client.tick_size(token).await?;
    /// let signable = client
    ///     .limit_order()
    ///     .token_id(token.parse::<U256>().unwrap())
    ///     .fee_rate_bps(fee.fee_rate_bps)
    ///     .minimum_tick_size(tick.minimum_tick_size)
    ///     .price(price)
    ///     .size(size)
    ///     .side(Side::Buy)
    ///     .maker(safe_address)
    ///     .build_and_sign(&signer)?;
    /// ```
    #[must_use]
    pub fn limit_order(&self) -> OrderBuilder<Limit> {
        OrderBuilder::<Limit>::limit()
    }

    /// Begin building a market order (FAK by default). See [`Self::limit_order`].
    #[must_use]
    pub fn market_order(&self) -> OrderBuilder<Market> {
        OrderBuilder::<Market>::market()
    }

    /// `POST /order` — submit a single signed order. L2-authenticated.
    pub async fn post_order(
        &self,
        signed: SignedOrder,
        order_type: crate::clob::types::OrderType,
        post_only: bool,
        owner: impl Into<String>,
    ) -> Result<PostOrderResponse> {
        let req = SendOrderRequest {
            order: signed,
            owner: owner.into(),
            order_type,
            post_only,
            defer_exec: false,
        };
        let body = serde_json::to_string(&req)?;
        let resp = self
            .request_authenticated(Method::POST, "/order", &[], Some(&body))
            .await?;
        let body = check_ok(resp, "POST", "/order").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /order response: {e}")))
    }

    /// `POST /orders` — batch up to 15 signed orders. L2-authenticated. Returns one
    /// [`PostOrderResponse`] per submitted order, preserving order.
    pub async fn post_orders(
        &self,
        signed: Vec<SignedOrder>,
        order_type: crate::clob::types::OrderType,
        post_only: bool,
        owner: impl Into<String> + Clone,
    ) -> Result<Vec<PostOrderResponse>> {
        if signed.is_empty() {
            return Ok(Vec::new());
        }
        if signed.len() > 15 {
            return Err(Error::validation(format!(
                "post_orders: chainup accepts at most 15 orders per batch (got {})",
                signed.len()
            )));
        }
        let owner = owner.into();
        let reqs: Vec<SendOrderRequest> = signed
            .into_iter()
            .map(|o| SendOrderRequest {
                order: o,
                owner: owner.clone(),
                order_type,
                post_only,
                defer_exec: false,
            })
            .collect();
        let body = serde_json::to_string(&reqs)?;
        let resp = self
            .request_authenticated(Method::POST, "/orders", &[], Some(&body))
            .await?;
        let body = check_ok(resp, "POST", "/orders").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /orders response: {e}")))
    }

    /// `POST /orders/replace` — atomic cancel + place. L2-authenticated.
    pub async fn replace_order(
        &self,
        req: ReplaceOrdersRequest,
    ) -> Result<ReplaceOrdersResponse> {
        let body = serde_json::to_string(&req)?;
        let resp = self
            .request_authenticated(Method::POST, "/orders/replace", &[], Some(&body))
            .await?;
        let body = check_ok(resp, "POST", "/orders/replace").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /orders/replace response: {e}")))
    }

    /// `DELETE /order` — cancel a single order by id. L2-authenticated.
    pub async fn cancel_order(&self, order_id: &str) -> Result<CancelOrdersResponse> {
        let body = serde_json::to_string(&serde_json::json!({ "orderID": order_id }))?;
        let resp = self
            .request_authenticated(Method::DELETE, "/order", &[], Some(&body))
            .await?;
        let body = check_ok(resp, "DELETE", "/order").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding DELETE /order response: {e}")))
    }

    /// `DELETE /orders` — batch cancel by id (max 3000). Sends the wire body as a bare
    /// JSON array, matching `services/clob-service/internal/tradingapi/handlers.CancelOrders`.
    /// L2-authenticated.
    pub async fn cancel_orders(&self, order_ids: &[String]) -> Result<CancelOrdersResponse> {
        if order_ids.len() > 3000 {
            return Err(Error::validation(format!(
                "cancel_orders: chainup accepts at most 3000 ids per batch (got {})",
                order_ids.len()
            )));
        }
        let body = serde_json::to_string(order_ids)?;
        let resp = self
            .request_authenticated(Method::DELETE, "/orders", &[], Some(&body))
            .await?;
        let body = check_ok(resp, "DELETE", "/orders").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding DELETE /orders response: {e}")))
    }

    /// `DELETE /cancel-all` — cancel every open order for the API-key owner.
    pub async fn cancel_all(&self) -> Result<CancelOrdersResponse> {
        let resp = self
            .request_authenticated(Method::DELETE, "/cancel-all", &[], None)
            .await?;
        let body = check_ok(resp, "DELETE", "/cancel-all").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /cancel-all response: {e}")))
    }

    /// `DELETE /cancel-market-orders` — cancel by condition id and/or token id. The server
    /// requires at least one of the two to be set.
    pub async fn cancel_market_orders(
        &self,
        request: CancelMarketOrderRequest,
    ) -> Result<CancelOrdersResponse> {
        if request.market.is_none() && request.asset_id.is_none() {
            return Err(Error::validation(
                "cancel_market_orders: at least one of `market` (condition id) or `asset_id` (token id) is required",
            ));
        }
        let body = serde_json::to_string(&request)?;
        let resp = self
            .request_authenticated(Method::DELETE, "/cancel-market-orders", &[], Some(&body))
            .await?;
        let body = check_ok(resp, "DELETE", "/cancel-market-orders").await?;
        serde_json::from_str(&body).map_err(|e| {
            Error::Validation(format!("decoding /cancel-market-orders response: {e}"))
        })
    }

    /// `GET /orders` — paginated open-order query. Pass `cursor` from a previous
    /// [`Page::next_cursor`] for forward pagination; pass `None` for the first page.
    pub async fn open_orders(
        &self,
        request: &OrdersRequest,
        cursor: Option<&str>,
    ) -> Result<Page<OpenOrderResponse>> {
        let mut query: Vec<(&str, String)> = Vec::with_capacity(6);
        if let Some(v) = &request.id {
            query.push(("id", v.clone()));
        }
        if let Some(v) = &request.market {
            query.push(("market", v.clone()));
        }
        if let Some(v) = &request.asset_id {
            query.push(("asset_id", v.clone()));
        }
        if let Some(v) = &request.status {
            query.push(("status", v.clone()));
        }
        if let Some(c) = cursor {
            query.push(("next_cursor", c.to_owned()));
        }
        let q_owned: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let resp = self
            .request_authenticated(Method::GET, "/orders", &q_owned, None)
            .await?;
        let body = check_ok(resp, "GET", "/orders").await?;
        serde_json::from_str::<Page<OpenOrderResponse>>(&body)
            .map_err(|e| Error::Validation(format!("decoding /orders response: {e}")))
    }

    /// `GET /order/{orderID}` — fetch a single order. Returns `Error::Api` with 404 when
    /// not found.
    pub async fn open_order(&self, order_id: &str) -> Result<OpenOrderResponse> {
        let path = format!("/order/{order_id}");
        let resp = self
            .request_authenticated(Method::GET, &path, &[], None)
            .await?;
        let body = check_ok(resp, "GET", &path).await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding {path} response: {e}")))
    }

    /// `GET /trades` — paginated trade query. The server's `maker_address` parameter is
    /// **required**; if the caller leaves [`TradesRequest::maker_address`] unset the SDK
    /// fills it with the configured L2 signer address.
    pub async fn trades(
        &self,
        request: &TradesRequest,
        cursor: Option<&str>,
    ) -> Result<Page<TradeResponse>> {
        let maker_addr = match request.maker_address.clone() {
            Some(a) => a,
            None => {
                let addr = self.require_signer_address()?;
                format!("{addr:#x}")
            }
        };
        let from_id_str;
        let limit_str;
        let before_str;
        let after_str;
        let mut query: Vec<(&str, &str)> = Vec::with_capacity(8);
        query.push(("maker_address", maker_addr.as_str()));
        if let Some(v) = &request.id {
            query.push(("id", v));
        }
        if let Some(v) = &request.market {
            query.push(("market", v));
        }
        if let Some(v) = &request.asset_id {
            query.push(("asset_id", v));
        }
        if let Some(v) = &request.before {
            before_str = v.to_string();
            query.push(("before", &before_str));
        }
        if let Some(v) = &request.after {
            after_str = v.to_string();
            query.push(("after", &after_str));
        }
        if let Some(v) = &request.from_id {
            from_id_str = v.to_string();
            query.push(("from_id", &from_id_str));
        }
        if let Some(v) = &request.limit {
            limit_str = v.to_string();
            query.push(("limit", &limit_str));
        }
        if let Some(c) = cursor {
            query.push(("next_cursor", c));
        }
        let resp = self
            .request_authenticated(Method::GET, "/trades", &query, None)
            .await?;
        let body = check_ok(resp, "GET", "/trades").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /trades response: {e}")))
    }

    /// `GET /builder/trades` — Builder-program variant of `/trades` with a 300-item limit.
    /// Takes the same [`TradesRequest`] filters.
    pub async fn builder_trades(
        &self,
        request: &TradesRequest,
        cursor: Option<&str>,
    ) -> Result<Page<TradeResponse>> {
        let from_id_str;
        let limit_str;
        let mut query: Vec<(&str, &str)> = Vec::with_capacity(6);
        if let Some(v) = &request.id {
            query.push(("id", v));
        }
        if let Some(v) = &request.market {
            query.push(("market", v));
        }
        if let Some(v) = &request.asset_id {
            query.push(("asset_id", v));
        }
        if let Some(v) = &request.from_id {
            from_id_str = v.to_string();
            query.push(("from_id", &from_id_str));
        }
        if let Some(v) = &request.limit {
            limit_str = v.to_string();
            query.push(("limit", &limit_str));
        }
        if let Some(c) = cursor {
            query.push(("next_cursor", c));
        }
        let resp = self
            .request_authenticated(Method::GET, "/builder/trades", &query, None)
            .await?;
        let body = check_ok(resp, "GET", "/builder/trades").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /builder/trades response: {e}")))
    }

    /// `GET /order-scoring` — check whether an order is eligible for maker-program rewards.
    pub async fn order_scoring(&self, order_id: &str) -> Result<OrderScoringResponse> {
        let resp = self
            .request_authenticated(Method::GET, "/order-scoring", &[("order_id", order_id)], None)
            .await?;
        let body = check_ok(resp, "GET", "/order-scoring").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /order-scoring response: {e}")))
    }

    /// Convenience wrapper that calls `/order-scoring` for each id and returns a map of
    /// `orderID -> scoring`. The server has no batch endpoint; each call is a separate
    /// HMAC-signed request.
    pub async fn orders_scoring(&self, ids: &[String]) -> Result<HashMap<String, bool>> {
        let mut out = HashMap::with_capacity(ids.len());
        for id in ids {
            let resp = self.order_scoring(id).await?;
            out.insert(id.clone(), resp.scoring);
        }
        Ok(out)
    }

    /// `POST /heartbeats` — keep maker-program orders alive (10-s timeout server-side).
    /// Send every 5 s.
    pub async fn heartbeat(&self) -> Result<HeartbeatResponse> {
        // Optional `heartbeat_id`; server accepts an empty JSON object too.
        let body = "{}";
        let resp = self
            .request_authenticated(Method::POST, "/heartbeats", &[], Some(body))
            .await?;
        let body = check_ok(resp, "POST", "/heartbeats").await?;
        serde_json::from_str(&body)
            .map_err(|e| Error::Validation(format!("decoding /heartbeats response: {e}")))
    }

    // ─── credentials / signer plumbing ──────────────────────────────────

    /// Reference to the configured [`Credentials`], or `None` when the client is
    /// unauthenticated.
    #[must_use]
    pub fn credentials(&self) -> Option<&Credentials> {
        self.inner.credentials.as_ref()
    }

    /// Return the configured credentials, or `Error::NotAuthenticated`.
    pub(crate) fn require_credentials(&self) -> Result<&Credentials> {
        self.inner.credentials.as_ref().ok_or(Error::NotAuthenticated)
    }

    /// EOA address for the L2 `PRED_ADDRESS` header. We hold the address rather than the
    /// signer because L2 calls never sign EIP-712 payloads.
    pub(crate) fn require_signer_address(&self) -> Result<crate::types::Address> {
        self.inner
            .signer_address
            .ok_or_else(|| Error::validation("L2 request requires signer address: call ClientBuilder::signer_address(...)"))
    }

    #[allow(dead_code)]
    pub(crate) fn http(&self) -> &HttpClient {
        &self.inner.http
    }

    // ─── Phase 3b: WebSocket sub-client ─────────────────────────────────

    /// Construct a [`crate::clob::ws::ClobWebSocketClient`] bound to this
    /// client's WS endpoint and (optionally) L2 credentials.
    ///
    /// The returned handle covers both the `/ws/market` (public) and
    /// `/ws/user` (auth-required) channels. Calling `subscribe_user` without
    /// credentials yields a validation error — attach them via
    /// [`ClientBuilder::credentials`].
    ///
    /// Errors with [`Error::Validation`] if no WS endpoint was configured
    /// (pass `--ws-endpoint` or `--tenant`).
    pub fn clob_ws(&self) -> Result<crate::clob::ws::ClobWebSocketClient> {
        let base = self.ws_url().ok_or_else(|| {
            Error::validation(
                "ws endpoint not configured: pass --ws-endpoint or use --tenant",
            )
        })?;
        Ok(crate::clob::ws::ClobWebSocketClient::new(
            base.clone(),
            self.inner.credentials.clone(),
        ))
    }
}

fn balance_allowance_query<'a>(
    asset_type: AssetType,
    token_id: Option<&'a str>,
) -> Result<Vec<(&'static str, &'a str)>> {
    let mut q: Vec<(&'static str, &'a str)> = Vec::with_capacity(2);
    q.push(("asset_type", asset_type.as_query_str()));
    match (asset_type, token_id) {
        (AssetType::Conditional, Some(t)) if !t.is_empty() => q.push(("token_id", t)),
        (AssetType::Conditional, _) => {
            return Err(Error::validation(
                "balance-allowance: token_id is required when asset_type=CONDITIONAL",
            ));
        }
        (AssetType::Collateral, Some(t)) if !t.is_empty() => {
            return Err(Error::validation(
                "balance-allowance: token_id must be omitted when asset_type=COLLATERAL",
            ));
        }
        (AssetType::Collateral, _) => {}
    }
    Ok(q)
}

async fn check_ok(resp: reqwest::Response, method: &'static str, path: &str) -> Result<String> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::api(status, method, path, body));
    }
    Ok(body)
}

/// Builder for [`Client`].
#[derive(Debug)]
pub struct ClientBuilder {
    endpoints: Option<Endpoints>,
    chain_id: Option<u64>,
    timeout: Duration,
    user_agent: String,
    credentials: Option<Credentials>,
    signer_address: Option<crate::types::Address>,
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
            signer_address: None,
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

    /// EOA address of the L1 signer that owns the configured [`Credentials`]. Required for
    /// any L2-authenticated call — the `PRED_ADDRESS` header is sent in every L2 request.
    #[must_use]
    pub fn signer_address(mut self, address: crate::types::Address) -> Self {
        self.signer_address = Some(address);
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
                signer_address: self.signer_address,
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
