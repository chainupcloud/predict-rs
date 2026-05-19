//! Typed stream wrappers around [`crate::ws::WsConnection`].
//!
//! Both [`MarketStream`] and [`UserStream`] implement
//! [`futures::Stream`] with `Item = Result<MarketEvent, WsError>` /
//! `Result<UserEvent, WsError>` respectively. A control handle on the stream
//! supports runtime subscribe / unsubscribe; dropping the stream tears down
//! the underlying connection.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use super::types::request::{
    MarketLevel, MarketSubscribeRequest, MarketUpdateRequest, SubscriptionOperation,
    UserSubscribeRequest, UserUpdateRequest,
};
use super::types::response::{MarketEvent, UserEvent};
use crate::ws::connection::{WsCommand, WsConnection, WsEvent};
use crate::ws::error::WsError;

/// Shared state describing the live subscription, used to re-emit on reconnect.
#[derive(Clone)]
pub(super) struct MarketState {
    pub(super) asset_ids: Arc<Mutex<Vec<String>>>,
    pub(super) initial_dump: bool,
    pub(super) level: Option<MarketLevel>,
    pub(super) custom_feature_enabled: Option<bool>,
}

/// Shared state describing the live user-channel subscription.
#[derive(Clone)]
pub(super) struct UserState {
    pub(super) auth: UserAuthClone,
    pub(super) markets: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
pub(super) struct UserAuthClone {
    pub(super) api_key: String,
    pub(super) passphrase: String,
}

/// Stream of [`MarketEvent`] frames with a control handle for runtime
/// subscribe / unsubscribe.
pub struct MarketStream {
    rx: mpsc::Receiver<Result<MarketEvent, WsError>>,
    cmd_tx: mpsc::Sender<WsCommand>,
    state: MarketState,
    /// Holding the connection here pins its lifetime to the stream; dropping
    /// the stream aborts the background task.
    _conn: WsConnection,
}

impl MarketStream {
    pub(super) fn new(
        rx: mpsc::Receiver<Result<MarketEvent, WsError>>,
        cmd_tx: mpsc::Sender<WsCommand>,
        state: MarketState,
        conn: WsConnection,
    ) -> Self {
        Self { rx, cmd_tx, state, _conn: conn }
    }

    /// Add more asset IDs to the active subscription. Idempotent — duplicates
    /// already present are tracked locally to keep the resubscribe-on-reconnect
    /// payload consistent.
    pub async fn subscribe(&self, asset_ids: Vec<String>) -> Result<(), WsError> {
        let mut current = self.state.asset_ids.lock().await;
        for id in &asset_ids {
            if !current.contains(id) {
                current.push(id.clone());
            }
        }
        drop(current);
        let req = MarketUpdateRequest {
            operation: SubscriptionOperation::Subscribe,
            assets_ids: asset_ids,
            level: self.state.level,
            custom_feature_enabled: self.state.custom_feature_enabled,
        };
        send_json(&self.cmd_tx, &req).await
    }

    /// Drop asset IDs from the active subscription.
    pub async fn unsubscribe(&self, asset_ids: Vec<String>) -> Result<(), WsError> {
        let mut current = self.state.asset_ids.lock().await;
        current.retain(|id| !asset_ids.contains(id));
        drop(current);
        let req = MarketUpdateRequest {
            operation: SubscriptionOperation::Unsubscribe,
            assets_ids: asset_ids,
            level: None,
            custom_feature_enabled: None,
        };
        send_json(&self.cmd_tx, &req).await
    }

    /// Close the underlying connection and stop receiving events.
    pub async fn close(&self) {
        let _ = self.cmd_tx.send(WsCommand::Close).await;
    }
}

impl Stream for MarketStream {
    type Item = Result<MarketEvent, WsError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Stream of [`UserEvent`] frames with a control handle for runtime
/// subscribe / unsubscribe by `market` (condition id).
pub struct UserStream {
    rx: mpsc::Receiver<Result<UserEvent, WsError>>,
    cmd_tx: mpsc::Sender<WsCommand>,
    state: UserState,
    _conn: WsConnection,
}

impl UserStream {
    pub(super) fn new(
        rx: mpsc::Receiver<Result<UserEvent, WsError>>,
        cmd_tx: mpsc::Sender<WsCommand>,
        state: UserState,
        conn: WsConnection,
    ) -> Self {
        Self { rx, cmd_tx, state, _conn: conn }
    }

    /// Add more condition IDs to the active subscription. Note: the chainup
    /// server interprets `subscribe` with a non-empty list as switching from
    /// "all markets" to filtered mode.
    pub async fn subscribe(&self, condition_ids: Vec<String>) -> Result<(), WsError> {
        let mut current = self.state.markets.lock().await;
        for cid in &condition_ids {
            if !current.contains(cid) {
                current.push(cid.clone());
            }
        }
        drop(current);
        let req = UserUpdateRequest {
            operation: SubscriptionOperation::Subscribe,
            markets: condition_ids,
        };
        send_json(&self.cmd_tx, &req).await
    }

    /// Drop condition IDs from the active subscription.
    pub async fn unsubscribe(&self, condition_ids: Vec<String>) -> Result<(), WsError> {
        let mut current = self.state.markets.lock().await;
        current.retain(|cid| !condition_ids.contains(cid));
        drop(current);
        let req = UserUpdateRequest {
            operation: SubscriptionOperation::Unsubscribe,
            markets: condition_ids,
        };
        send_json(&self.cmd_tx, &req).await
    }

    /// Close the underlying connection.
    pub async fn close(&self) {
        let _ = self.cmd_tx.send(WsCommand::Close).await;
    }
}

impl Stream for UserStream {
    type Item = Result<UserEvent, WsError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

async fn send_json<T: Serialize>(tx: &mpsc::Sender<WsCommand>, value: &T) -> Result<(), WsError> {
    let json = serde_json::to_string(value)?;
    tx.send(WsCommand::Send(json))
        .await
        .map_err(|_| WsError::Internal("ws connection task is gone".into()))
}

// ─── pump tasks ────────────────────────────────────────────────────────────

pub(super) fn spawn_market_pump(
    mut evt_rx: mpsc::Receiver<WsEvent>,
    out_tx: mpsc::Sender<Result<MarketEvent, WsError>>,
    cmd_tx: mpsc::Sender<WsCommand>,
    state: MarketState,
) {
    tokio::spawn(async move {
        while let Some(ev) = evt_rx.recv().await {
            match ev {
                WsEvent::Connected => {
                    let ids = state.asset_ids.lock().await.clone();
                    if ids.is_empty() {
                        continue;
                    }
                    let req = MarketSubscribeRequest {
                        assets_ids: ids,
                        r#type: "market",
                        initial_dump: Some(state.initial_dump),
                        level: state.level,
                        custom_feature_enabled: state.custom_feature_enabled,
                    };
                    if let Ok(payload) = serde_json::to_string(&req) {
                        if cmd_tx.send(WsCommand::Send(payload)).await.is_err() {
                            return;
                        }
                    }
                }
                WsEvent::Message(text) => match serde_json::from_str::<MarketEvent>(&text) {
                    Ok(parsed) => {
                        if out_tx.send(Ok(parsed)).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = out_tx
                            .send(Err(WsError::Decode(format!(
                                "{e}; raw frame (truncated 256B): {}",
                                truncate(&text, 256)
                            ))))
                            .await;
                    }
                },
                WsEvent::Reconnecting { .. } | WsEvent::Disconnected => {}
                WsEvent::Error(e) => {
                    let _ = out_tx.send(Err(e)).await;
                    return;
                }
            }
        }
    });
}

pub(super) fn spawn_user_pump(
    mut evt_rx: mpsc::Receiver<WsEvent>,
    out_tx: mpsc::Sender<Result<UserEvent, WsError>>,
    cmd_tx: mpsc::Sender<WsCommand>,
    state: UserState,
) {
    tokio::spawn(async move {
        while let Some(ev) = evt_rx.recv().await {
            match ev {
                WsEvent::Connected => {
                    let markets = state.markets.lock().await.clone();
                    let req = UserSubscribeRequest::new(
                        state.auth.api_key.clone(),
                        state.auth.passphrase.clone(),
                        markets,
                    );
                    if let Ok(payload) = serde_json::to_string(&req) {
                        if cmd_tx.send(WsCommand::Send(payload)).await.is_err() {
                            return;
                        }
                    }
                }
                WsEvent::Message(text) => {
                    if let Ok(map) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(err) = map.get("error").and_then(|v| v.as_str()) {
                            let _ = out_tx
                                .send(Err(WsError::UserAuthRejected(err.into())))
                                .await;
                            return;
                        }
                    }
                    match serde_json::from_str::<UserEvent>(&text) {
                        Ok(parsed) => {
                            if out_tx.send(Ok(parsed)).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = out_tx
                                .send(Err(WsError::Decode(format!(
                                    "{e}; raw frame (truncated 256B): {}",
                                    truncate(&text, 256)
                                ))))
                                .await;
                        }
                    }
                }
                WsEvent::Reconnecting { .. } | WsEvent::Disconnected => {}
                WsEvent::Error(e) => {
                    let _ = out_tx.send(Err(e)).await;
                    return;
                }
            }
        }
    });
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_owned()
    } else {
        let mut out = s[..n].to_owned();
        out.push_str("…");
        out
    }
}

// ─── constructors used by client.rs ────────────────────────────────────────

pub(super) fn new_market_state(
    asset_ids: Vec<String>,
    initial_dump: bool,
    level: Option<MarketLevel>,
    custom_feature_enabled: Option<bool>,
) -> MarketState {
    MarketState {
        asset_ids: Arc::new(Mutex::new(asset_ids)),
        initial_dump,
        level,
        custom_feature_enabled,
    }
}

pub(super) fn new_user_state(
    api_key: String,
    passphrase: String,
    markets: Vec<String>,
) -> UserState {
    UserState {
        auth: UserAuthClone { api_key, passphrase },
        markets: Arc::new(Mutex::new(markets)),
    }
}
