//! [`ClobWebSocketClient`] — entry point for the two WS channels.
//!
//! Construct via [`crate::Client::clob_ws`]; that method validates the parent
//! client has a WS endpoint configured. The two `subscribe_*` methods each
//! dial a fresh socket — both channels run on `:8082` but at
//! different paths (`/ws/market` vs `/ws/user`).

use std::pin::Pin;

use futures::{Stream, StreamExt as _};
use rust_decimal::Decimal;
use secrecy::ExposeSecret as _;
use tokio::sync::mpsc;
use url::Url;

use super::subscription::{self, MarketStream, UserStream};
use super::types::request::MarketLevel;
use super::types::response::{
    BestBidAskEvent, BookEvent, LastTradePriceEvent, MarketEvent, MarketResolvedEvent,
    NewMarketEvent, OrderEvent, PriceChangeEvent, TickSizeChangeEvent, TradeEvent, UserEvent,
};
use crate::auth::Credentials;
use crate::error::{Error, Result};
use crate::ws::{WsConfig, WsConnection, WsError};

/// Optional knobs for [`ClobWebSocketClient::subscribe_market`].
#[derive(Debug, Clone, Default)]
pub struct MarketSubscribeOpts {
    /// If `Some(false)`, the server skips the per-asset `book` snapshot dump
    /// immediately after subscribe. Default (server-side): true.
    pub initial_dump: Option<bool>,
    /// Order-book depth level. `None` lets the server default (`level=2`).
    pub level: Option<MarketLevel>,
    /// Opt-in to the `best_bid_ask` / `new_market` / `market_resolved` events.
    pub custom_feature_enabled: Option<bool>,
}

impl MarketSubscribeOpts {
    #[must_use]
    pub fn with_initial_dump(mut self, dump: bool) -> Self {
        self.initial_dump = Some(dump);
        self
    }
    #[must_use]
    pub fn with_level(mut self, level: MarketLevel) -> Self {
        self.level = Some(level);
        self
    }
    #[must_use]
    pub fn with_custom_features(mut self, on: bool) -> Self {
        self.custom_feature_enabled = Some(on);
        self
    }
}

/// CLOB-channel WebSocket sub-client.
///
/// Sub-clients borrow shared state from the parent [`crate::Client`] but own
/// their own WS connections.
#[derive(Clone, Debug)]
pub struct ClobWebSocketClient {
    base: Url,
    credentials: Option<Credentials>,
    config: WsConfig,
}

impl ClobWebSocketClient {
    /// Construct directly. Most callers should use [`crate::Client::clob_ws`].
    #[must_use]
    pub fn new(base: Url, credentials: Option<Credentials>) -> Self {
        Self { base, credentials, config: WsConfig::default() }
    }

    /// Override the connection config (heartbeat / backoff / etc.).
    #[must_use]
    pub fn with_config(mut self, config: WsConfig) -> Self {
        self.config = config;
        self
    }

    /// Base WS URL (e.g. `wss://clob-ws.hermestrade.xyz/`).
    #[must_use]
    pub fn base(&self) -> &Url {
        &self.base
    }

    /// Subscribe to `/ws/market` for one or more asset (token) IDs. Returns a
    /// [`MarketStream`] yielding [`MarketEvent`] frames; runtime
    /// subscribe / unsubscribe is supported via the stream's methods.
    pub async fn subscribe_market(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<MarketStream> {
        if asset_ids.is_empty() {
            return Err(Error::validation("subscribe_market: asset_ids must be non-empty"));
        }
        let url = self.join("ws/market")?;
        let (conn, evt_rx, cmd_tx) = WsConnection::dial(url, self.config.clone());

        let state = subscription::new_market_state(
            asset_ids,
            opts.initial_dump.unwrap_or(true),
            opts.level,
            opts.custom_feature_enabled,
        );

        let (out_tx, out_rx) =
            mpsc::channel::<std::result::Result<MarketEvent, WsError>>(self.config.channel_capacity);
        subscription::spawn_market_pump(evt_rx, out_tx, cmd_tx.clone(), state.clone());
        Ok(MarketStream::new(out_rx, cmd_tx, state, conn))
    }

    /// Subscribe to `/ws/user`. `condition_ids` empty = subscribe to all
    /// markets owned by the authenticated API key.
    pub async fn subscribe_user(&self, condition_ids: Vec<String>) -> Result<UserStream> {
        let creds = self.credentials.as_ref().ok_or_else(|| {
            Error::validation(
                "subscribe_user: L2 credentials not attached. \
                 Build the parent Client with ClientBuilder::credentials(...)",
            )
        })?;
        let url = self.join("ws/user")?;
        let (conn, evt_rx, cmd_tx) = WsConnection::dial(url, self.config.clone());

        let state = subscription::new_user_state(
            creds.key.to_string(),
            creds.passphrase.expose_secret().to_string(),
            condition_ids,
        );

        let (out_tx, out_rx) =
            mpsc::channel::<std::result::Result<UserEvent, WsError>>(self.config.channel_capacity);
        subscription::spawn_user_pump(evt_rx, out_tx, cmd_tx.clone(), state.clone());
        Ok(UserStream::new(out_rx, cmd_tx, state, conn))
    }

    /// Convenience: connect, send one `PING`, await `PONG`, disconnect.
    ///
    /// Returns `Ok(())` if the server accepts the upgrade within `timeout`
    /// (the server's response to `"PING"` is the text frame `"PONG"`,
    /// which the connection task swallows; absence of a transport error
    /// within ~200 ms after the PING is treated as success). Used by
    /// `predict-cli ws ping`.
    pub async fn ping(&self, timeout: std::time::Duration) -> Result<()> {
        let url = self.join("ws/market")?;
        let mut cfg = self.config.clone();
        cfg.ping_interval = std::time::Duration::ZERO;
        cfg.emit_reconnecting = false;
        let (conn, mut evt_rx, cmd_tx) = WsConnection::dial(url, cfg);

        let connect_deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(connect_deadline, evt_rx.recv()).await {
                Ok(Some(crate::ws::WsEvent::Connected)) => break,
                Ok(Some(crate::ws::WsEvent::Error(e))) => return Err(e.into()),
                Ok(Some(_)) => continue,
                Ok(None) => return Err(WsError::Internal("ws task ended".into()).into()),
                Err(_) => return Err(WsError::Transport("connect timeout".into()).into()),
            }
        }

        cmd_tx
            .send(crate::ws::connection::WsCommand::Send("PING".into()))
            .await
            .map_err(|_| WsError::Internal("send PING: task gone".into()))?;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        conn.close().await;
        Ok(())
    }

    fn join(&self, suffix: &str) -> Result<Url> {
        let trimmed = suffix.trim_start_matches('/');
        Ok(self.base.join(trimmed)?)
    }
}

// ─── typed-filter helpers ──────────────────────────────────────────────────
//
// Pattern lifted from the upstream `rs-clob-client::clob::ws::client` — each
// method spins up the underlying channel stream and `filter_map`s to keep only
// the variant the caller cares about. Errors propagate untouched. Use these
// when you want a typed stream; use `subscribe_market` / `subscribe_user`
// directly when you need the control handle (runtime subscribe / unsubscribe).

/// Filter the next `MarketEvent` for the requested variant. Used by the typed
/// helpers below to avoid 6 near-identical closures.
macro_rules! market_filter {
    ($variant:path) => {
        |res| async move {
            match res {
                Ok($variant(inner)) => Some(Ok(inner)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            }
        }
    };
}

macro_rules! user_filter {
    ($variant:path) => {
        |res| async move {
            match res {
                Ok($variant(inner)) => Some(Ok(inner)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            }
        }
    };
}

/// `(bid + ask) / 2` snapshot computed from a [`BookEvent`].
///
/// Mirrors the upstream `MidpointUpdate`. Emitted only when both sides have at
/// least one level — frames where one side is empty are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct MidpointUpdate {
    pub asset_id: String,
    pub market: String,
    pub midpoint: Decimal,
    pub timestamp: super::types::response::Timestamp,
}

impl ClobWebSocketClient {
    /// Subscribe and yield only `book` frames. See [`Self::subscribe_market`]
    /// when you also want `price_change` / `last_trade_price` / etc.
    pub async fn subscribe_orderbook(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<BookEvent, WsError>> + Send>>> {
        let stream = self.subscribe_market(asset_ids, opts).await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::Book))))
    }

    /// Subscribe and yield only `price_change` frames.
    pub async fn subscribe_prices(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<PriceChangeEvent, WsError>> + Send>>> {
        let stream = self.subscribe_market(asset_ids, opts).await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::PriceChange))))
    }

    /// Subscribe and yield only `last_trade_price` frames.
    pub async fn subscribe_last_trade_price(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<LastTradePriceEvent, WsError>> + Send>>> {
        let stream = self.subscribe_market(asset_ids, opts).await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::LastTradePrice))))
    }

    /// Subscribe and yield only `tick_size_change` frames.
    pub async fn subscribe_tick_size_change(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<TickSizeChangeEvent, WsError>> + Send>>> {
        let stream = self.subscribe_market(asset_ids, opts).await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::TickSizeChange))))
    }

    /// Subscribe and yield only `best_bid_ask` frames. Requires the server-side
    /// `custom_feature_enabled` flag, so the helper forces `opts.with_custom_features(true)`.
    pub async fn subscribe_best_bid_ask(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<BestBidAskEvent, WsError>> + Send>>> {
        let stream = self
            .subscribe_market(asset_ids, opts.with_custom_features(true))
            .await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::BestBidAsk))))
    }

    /// Subscribe and yield only `new_market` frames. Requires `custom_feature_enabled`.
    pub async fn subscribe_new_markets(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<NewMarketEvent, WsError>> + Send>>> {
        let stream = self
            .subscribe_market(asset_ids, opts.with_custom_features(true))
            .await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::NewMarket))))
    }

    /// Subscribe and yield only `market_resolved` frames. Requires `custom_feature_enabled`.
    pub async fn subscribe_market_resolutions(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<MarketResolvedEvent, WsError>> + Send>>> {
        let stream = self
            .subscribe_market(asset_ids, opts.with_custom_features(true))
            .await?;
        Ok(Box::pin(stream.filter_map(market_filter!(MarketEvent::MarketResolved))))
    }

    /// Subscribe and emit `(best_bid + best_ask) / 2` midpoint updates computed from `book`
    /// frames. Frames with one side empty are dropped. Mirrors the upstream
    /// `subscribe_midpoints`.
    pub async fn subscribe_midpoints(
        &self,
        asset_ids: Vec<String>,
        opts: MarketSubscribeOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<MidpointUpdate, WsError>> + Send>>> {
        let stream = self.subscribe_orderbook(asset_ids, opts).await?;
        Ok(Box::pin(stream.filter_map(|res| async move {
            match res {
                Ok(book) => {
                    let (Some(bid), Some(ask)) = (book.bids.first(), book.asks.first()) else {
                        return None;
                    };
                    let bid_p: Decimal = bid.price.parse().ok()?;
                    let ask_p: Decimal = ask.price.parse().ok()?;
                    Some(Ok(MidpointUpdate {
                        asset_id: book.asset_id,
                        market: book.market,
                        midpoint: (bid_p + ask_p) / Decimal::TWO,
                        timestamp: book.timestamp,
                    }))
                }
                Err(e) => Some(Err(e)),
            }
        })))
    }

    /// Subscribe and yield only `order` events from the user channel.
    pub async fn subscribe_orders(
        &self,
        condition_ids: Vec<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<OrderEvent, WsError>> + Send>>> {
        let stream = self.subscribe_user(condition_ids).await?;
        Ok(Box::pin(stream.filter_map(user_filter!(UserEvent::Order))))
    }

    /// Subscribe and yield only `trade` events from the user channel.
    pub async fn subscribe_trades(
        &self,
        condition_ids: Vec<String>,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<TradeEvent, WsError>> + Send>>> {
        let stream = self.subscribe_user(condition_ids).await?;
        Ok(Box::pin(stream.filter_map(user_filter!(UserEvent::Trade))))
    }
}
