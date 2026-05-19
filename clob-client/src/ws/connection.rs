//! Thin wrapper over `tokio_tungstenite` providing:
//!
//! - A single-connection dial helper that surfaces HTTP auth failures
//!   distinctly from transient transport errors.
//! - A text-frame heartbeat loop that sends the literal string `"PING"` at
//!   [`WsConfig::ping_interval`] and treats incoming `"PONG"` as keep-alives
//!   (the chainup server expects PING/PONG as **text** frames, not the
//!   WebSocket protocol-level Ping opcode — see
//!   `services/clob-service/internal/wsservice/market_channel.go`).
//! - A typed event stream surfacing [`WsEvent::Connected`] /
//!   [`WsEvent::Message`] / [`WsEvent::Reconnecting`] /
//!   [`WsEvent::Disconnected`] / [`WsEvent::Error`].
//!
//! The CLOB-specific subscription mechanics (preserving subscriptions across
//! reconnects, splitting into market / user streams) live in
//! [`crate::clob::ws`].

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use url::Url;

use super::config::WsConfig;
use super::error::WsError;

/// Events emitted by the reconnect loop.
#[derive(Debug)]
pub enum WsEvent {
    /// A WebSocket connection has been established. Carries any control
    /// frames the higher-level client should re-send (e.g. resubscribe).
    Connected,
    /// A text frame arrived. Binary frames are silently dropped; `"PONG"`
    /// keep-alives are consumed by the connection task and not surfaced.
    Message(String),
    /// The connection loop is about to dial again after a transient failure.
    /// The higher-level client should treat any cached state as stale.
    Reconnecting { attempt: u32, after: Duration },
    /// The remote closed the socket cleanly.
    Disconnected,
    /// Terminal error from the reconnect loop. Always the last event.
    Error(WsError),
}

/// Outbound request sent from the higher-level client to the live socket.
///
/// `Send` carries the JSON-text payload (or the literal `"PING"` keep-alive);
/// `Close` triggers a graceful close + reconnect-loop exit.
#[derive(Debug, Clone)]
pub enum WsCommand {
    Send(String),
    Close,
}

/// Single-connection reconnect loop. Construct with [`Self::dial`] and consume
/// events from the returned [`mpsc::Receiver`].
///
/// The connection runs entirely on a background task; dropping the handle
/// cancels it.
pub struct WsConnection {
    cmd_tx: mpsc::Sender<WsCommand>,
    handle: tokio::task::JoinHandle<()>,
}

impl WsConnection {
    /// Dial the URL and spin up the reconnect loop. Returns immediately —
    /// the first [`WsEvent::Connected`] event signals that the upgrade has
    /// completed.
    ///
    /// `extra_headers` is currently empty for both chainup channels but kept
    /// in the signature so we can attach `User-Agent` / proxy headers later.
    #[must_use = "dropping the WsConnection cancels the background task"]
    pub fn dial(
        url: Url,
        config: WsConfig,
    ) -> (Self, mpsc::Receiver<WsEvent>, mpsc::Sender<WsCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(config.channel_capacity);
        let (evt_tx, evt_rx) = mpsc::channel::<WsEvent>(config.channel_capacity);
        let cmd_tx_clone = cmd_tx.clone();
        let handle = tokio::spawn(reconnect_loop(url, config, evt_tx, cmd_rx, cmd_tx.clone()));
        (
            Self {
                cmd_tx: cmd_tx_clone,
                handle,
            },
            evt_rx,
            cmd_tx,
        )
    }

    /// Send a text frame.
    pub async fn send(&self, payload: String) -> Result<(), WsError> {
        self.cmd_tx
            .send(WsCommand::Send(payload))
            .await
            .map_err(|_| WsError::Internal("connection task is gone".into()))
    }

    /// Request graceful close + cancel the background task.
    pub async fn close(&self) {
        let _ = self.cmd_tx.send(WsCommand::Close).await;
    }

    /// Abort the background task (does not wait for graceful close).
    pub fn abort(&self) {
        self.handle.abort();
    }
}

impl Drop for WsConnection {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn reconnect_loop(
    url: Url,
    config: WsConfig,
    evt_tx: mpsc::Sender<WsEvent>,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
    self_tx: mpsc::Sender<WsCommand>,
) {
    let mut attempt: u32 = 0;
    let mut backoff = config.initial_backoff;

    loop {
        // Dial.
        let request = match url.as_str().into_client_request() {
            Ok(req) => req,
            Err(e) => {
                let _ = evt_tx.send(WsEvent::Error(WsError::Connect(e.to_string()))).await;
                return;
            }
        };
        let dial_fut = tokio_tungstenite::connect_async(request);
        let dial_result = tokio::time::timeout(config.connect_timeout, dial_fut).await;
        let conn = match dial_result {
            Ok(Ok((conn, _resp))) => conn,
            Ok(Err(e)) => {
                let err: WsError = e.into();
                if err.is_fatal() {
                    let _ = evt_tx.send(WsEvent::Error(err)).await;
                    return;
                }
                if !schedule_reconnect(&evt_tx, &config, &mut attempt, &mut backoff).await {
                    return;
                }
                continue;
            }
            Err(_) => {
                if !schedule_reconnect(&evt_tx, &config, &mut attempt, &mut backoff).await {
                    return;
                }
                continue;
            }
        };

        // Connected — reset backoff.
        attempt = 0;
        backoff = config.initial_backoff;
        if evt_tx.send(WsEvent::Connected).await.is_err() {
            return;
        }

        // Spawn heartbeat task.
        let (ws_tx, mut ws_rx) = conn.split();
        let writer = std::sync::Arc::new(tokio::sync::Mutex::new(ws_tx));
        let heartbeat = spawn_heartbeat(writer.clone(), config.ping_interval, self_tx.clone());

        // Process commands + inbound frames concurrently.
        let exit_reason = run_session(&mut cmd_rx, &mut ws_rx, writer.clone(), &evt_tx).await;
        heartbeat.abort();
        // Best-effort close.
        let _ = writer.lock().await.close().await;

        match exit_reason {
            SessionExit::ClientClose => {
                let _ = evt_tx.send(WsEvent::Disconnected).await;
                return;
            }
            SessionExit::RemoteClose => {
                let _ = evt_tx.send(WsEvent::Disconnected).await;
                // fall through to reconnect
            }
            SessionExit::Error(e) => {
                if e.is_fatal() {
                    let _ = evt_tx.send(WsEvent::Error(e)).await;
                    return;
                }
                let _ = evt_tx.send(WsEvent::Disconnected).await;
            }
            SessionExit::ChannelClosed => {
                // The higher-level client dropped its event receiver.
                return;
            }
        }

        if !schedule_reconnect(&evt_tx, &config, &mut attempt, &mut backoff).await {
            return;
        }
    }
}

enum SessionExit {
    ClientClose,
    RemoteClose,
    Error(WsError),
    ChannelClosed,
}

async fn run_session<S>(
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
    ws_rx: &mut futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<S>>,
    writer: std::sync::Arc<
        tokio::sync::Mutex<
            futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<S>, Message>,
        >,
    >,
    evt_tx: &mpsc::Sender<WsEvent>,
) -> SessionExit
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(WsCommand::Send(payload)) => {
                    if writer.lock().await.send(Message::Text(payload.into())).await.is_err() {
                        return SessionExit::Error(WsError::Transport("send failed".into()));
                    }
                }
                Some(WsCommand::Close) => return SessionExit::ClientClose,
                None => return SessionExit::ChannelClosed,
            },
            frame = ws_rx.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    let s: String = text.to_string();
                    if s == "PONG" {
                        continue;
                    }
                    if evt_tx.send(WsEvent::Message(s)).await.is_err() {
                        return SessionExit::ChannelClosed;
                    }
                }
                Some(Ok(Message::Binary(_))) => {
                    // chainup never emits binary frames; ignore.
                }
                Some(Ok(Message::Ping(payload))) => {
                    // Reply at the protocol level if the server uses real ping frames.
                    let _ = writer.lock().await.send(Message::Pong(payload)).await;
                }
                Some(Ok(Message::Pong(_))) => {}
                Some(Ok(Message::Frame(_))) => {}
                Some(Ok(Message::Close(_))) => return SessionExit::RemoteClose,
                Some(Err(e)) => return SessionExit::Error(e.into()),
                None => return SessionExit::RemoteClose,
            }
        }
    }
}

fn spawn_heartbeat<S>(
    writer: std::sync::Arc<
        tokio::sync::Mutex<
            futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<S>, Message>,
        >,
    >,
    interval: Duration,
    _self_tx: mpsc::Sender<WsCommand>,
) -> tokio::task::JoinHandle<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        if interval.is_zero() {
            return;
        }
        let mut ticker = tokio::time::interval(interval);
        // Skip the immediate first tick.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            // Send the chainup text-frame heartbeat. The server replies with
            // the text "PONG" which the reader silently consumes.
            let mut guard = writer.lock().await;
            let sink: &mut futures_util::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<S>,
                Message,
            > = &mut guard;
            if sink.send(Message::Text("PING".into())).await.is_err() {
                return;
            }
        }
    })
}

async fn schedule_reconnect(
    evt_tx: &mpsc::Sender<WsEvent>,
    config: &WsConfig,
    attempt: &mut u32,
    backoff: &mut Duration,
) -> bool {
    *attempt = attempt.saturating_add(1);
    let wait = *backoff;
    if config.emit_reconnecting
        && evt_tx
            .send(WsEvent::Reconnecting { attempt: *attempt, after: wait })
            .await
            .is_err()
    {
        return false;
    }
    tokio::time::sleep(wait).await;
    let next = backoff.saturating_mul(2);
    *backoff = if next > config.max_backoff { config.max_backoff } else { next };
    true
}
