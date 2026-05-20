//! Offline end-to-end tests for [`pm_rs_clob_client::ClobWebSocketClient`].
//!
//! We spin a minimal `tokio_tungstenite` server on a free port, drive the
//! SDK against it, and assert the wire frames are exactly what the chainup
//! `clob-service` would see.
//!
//! These tests do NOT touch the network and run on every `cargo test`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures_util::SinkExt as _;
use pm_rs_clob_client::clob::ws::types::request::MarketLevel;
use pm_rs_clob_client::clob::ws::types::response::{MarketEvent, OrderSide, UserEvent};
use pm_rs_clob_client::{
    Client, ClobWebSocketClient, Credentials, Endpoints, MarketSubscribeOpts,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use uuid::Uuid;

/// Capture of every frame received by the server and every frame written.
#[derive(Default)]
struct ServerLog {
    received: Vec<String>,
}

type SharedLog = Arc<Mutex<ServerLog>>;

/// Spin up a single-connection echo server that:
///
/// 1. Captures the first subscribe envelope.
/// 2. Sends back the configured push frames in order.
/// 3. Stays open until the client closes (so the client's reconnect loop
///    sees a clean disconnect rather than an immediate retry storm).
///
/// Returns the bound URL and a handle for inspecting captured frames.
async fn spin_server(push_frames: Vec<String>) -> (Url, SharedLog) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let log: SharedLog = Arc::default();
    let log_clone = log.clone();

    tokio::spawn(async move {
        // Accept exactly one connection then exit.
        let (stream, _peer) = match listener.accept().await {
            Ok(p) => p,
            Err(_) => return,
        };
        let mut ws = match accept_async(stream).await {
            Ok(w) => w,
            Err(_) => return,
        };

        // Push frames first so the SDK has something to decode before
        // we wait for outbound (subscribe) frames. The chainup market server
        // would not do this, but the SDK doesn't care about ordering — the
        // pump reads all inbound frames concurrently with sending.
        for frame in push_frames {
            if ws.send(Message::Text(frame.into())).await.is_err() {
                return;
            }
        }

        while let Some(msg) = ws.next().await {
            let Ok(msg) = msg else { return };
            match msg {
                Message::Text(t) => {
                    let s: String = t.to_string();
                    log_clone.lock().await.received.push(s.clone());
                    if s == "PING" {
                        let _ = ws.send(Message::Text("PONG".into())).await;
                    }
                }
                Message::Close(_) => return,
                _ => {}
            }
        }
    });

    let url = Url::parse(&format!("ws://{local}/")).unwrap();
    let _: SocketAddr = local;
    (url, log)
}

fn build_ws_client(base: Url, creds: Option<Credentials>) -> ClobWebSocketClient {
    let cfg = pm_rs_clob_client::ws::WsConfig {
        ping_interval: Duration::ZERO, // disable heartbeat for deterministic tests
        emit_reconnecting: false,
        ..Default::default()
    };
    ClobWebSocketClient::new(base, creds).with_config(cfg)
}

#[tokio::test]
async fn subscribe_market_serializes_correct_envelope_and_decodes_book() {
    let book_frame = r#"{
        "event_type": "book",
        "asset_id": "asset-1",
        "data": {
            "market": "0xmarket",
            "asset_id": "asset-1",
            "bids": [{"price": "0.4", "size": "10"}],
            "asks": [{"price": "0.6", "size": "20"}],
            "timestamp": 1700000000,
            "hash": "0xdead"
        }
    }"#
    .to_owned();

    let (url, log) = spin_server(vec![book_frame]).await;
    let ws = build_ws_client(url, None);

    let opts = MarketSubscribeOpts::default()
        .with_initial_dump(true)
        .with_level(MarketLevel::Two);
    let mut stream = ws
        .subscribe_market(vec!["asset-1".into(), "asset-2".into()], opts)
        .await
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("event arrived in time")
        .expect("stream not closed")
        .expect("decoded ok");
    match event {
        MarketEvent::Book(b) => {
            assert_eq!(b.asset_id, "asset-1");
            assert_eq!(b.bids[0].size, "10");
            assert_eq!(b.asks[0].price, "0.6");
        }
        other => panic!("wrong variant: {other:?}"),
    }

    // Verify the subscribe envelope reached the server.
    // Wait briefly for the pump to write the subscribe frame.
    for _ in 0..20 {
        if !log.lock().await.received.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let frames = log.lock().await.received.clone();
    assert!(!frames.is_empty(), "server should have seen at least one frame");
    let sub: serde_json::Value = serde_json::from_str(&frames[0]).unwrap();
    assert_eq!(sub["type"], "market");
    assert_eq!(sub["assets_ids"][0], "asset-1");
    assert_eq!(sub["assets_ids"][1], "asset-2");
    assert_eq!(sub["initial_dump"], true);
    assert_eq!(sub["level"], 2);
}

#[tokio::test]
async fn market_runtime_subscribe_unsubscribe_round_trip() {
    let (url, log) = spin_server(vec![]).await;
    let ws = build_ws_client(url, None);
    let stream = ws
        .subscribe_market(vec!["a".into()], MarketSubscribeOpts::default())
        .await
        .unwrap();

    // First wait for the initial subscribe frame to arrive.
    for _ in 0..40 {
        if !log.lock().await.received.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    stream.subscribe(vec!["b".into()]).await.unwrap();
    stream.unsubscribe(vec!["a".into()]).await.unwrap();

    // Drain frames; need to wait a beat for them to be written.
    for _ in 0..40 {
        if log.lock().await.received.len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let frames = log.lock().await.received.clone();
    assert!(
        frames.len() >= 3,
        "expected initial subscribe + subscribe(b) + unsubscribe(a); got {frames:?}"
    );
    let subscribe_b: serde_json::Value = serde_json::from_str(&frames[1]).unwrap();
    let unsubscribe_a: serde_json::Value = serde_json::from_str(&frames[2]).unwrap();
    assert_eq!(subscribe_b["operation"], "subscribe");
    assert_eq!(subscribe_b["assets_ids"][0], "b");
    assert_eq!(unsubscribe_a["operation"], "unsubscribe");
    assert_eq!(unsubscribe_a["assets_ids"][0], "a");
}

#[tokio::test]
async fn subscribe_user_carries_auth_in_first_frame() {
    let order_frame = r#"{
        "event_type": "order",
        "owner": "owner",
        "condition_id": "0xcid",
        "data": {
            "id": "0xorder",
            "asset_id": "1",
            "side": "BUY",
            "original_size": "10",
            "size_matched": "0",
            "price": "0.5",
            "type": "GTC",
            "status": "live"
        }
    }"#
    .to_owned();
    let (url, log) = spin_server(vec![order_frame]).await;

    let creds = Credentials::new(
        Uuid::nil(),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        "pass-1".into(),
    );
    let ws = build_ws_client(url, Some(creds));
    let mut stream = ws.subscribe_user(vec!["0xcid".into()]).await.unwrap();

    // Receive the pushed order frame.
    let ev = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    match ev {
        UserEvent::Order(o) => {
            assert_eq!(o.side, Some(OrderSide::Buy));
        }
        UserEvent::Trade(_) => panic!("wrong variant"),
    }

    // Wait for the subscribe frame to be logged.
    for _ in 0..40 {
        if !log.lock().await.received.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let frames = log.lock().await.received.clone();
    assert!(!frames.is_empty(), "server should have seen the auth frame");
    let sub: serde_json::Value = serde_json::from_str(&frames[0]).unwrap();
    assert_eq!(sub["type"], "user");
    assert_eq!(sub["auth"]["apiKey"], Uuid::nil().to_string());
    assert_eq!(sub["auth"]["passphrase"], "pass-1");
    assert_eq!(sub["markets"][0], "0xcid");
}

#[tokio::test]
async fn user_channel_auth_failure_surfaces_as_user_auth_rejected() {
    let err_frame = r#"{"error":"authentication failed"}"#.to_owned();
    let (url, _) = spin_server(vec![err_frame]).await;

    let creds = Credentials::new(Uuid::nil(), "AAAA".into(), "bad".into());
    let ws = build_ws_client(url, Some(creds));
    let mut stream = ws.subscribe_user(vec![]).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap();
    let err = next.expect_err("expected user-auth error envelope");
    assert!(
        matches!(err, pm_rs_clob_client::ws::WsError::UserAuthRejected(_)),
        "got: {err:?}"
    );
}

#[tokio::test]
async fn client_clob_ws_requires_ws_endpoint() {
    let client = Client::builder()
        .endpoints(Endpoints::clob_only("https://example.com").unwrap())
        .build()
        .unwrap();
    let err = client.clob_ws().expect_err("ws endpoint missing should error");
    let msg = err.to_string();
    assert!(msg.contains("ws endpoint not configured"), "{msg}");
}

#[tokio::test]
async fn typed_orderbook_helper_filters_to_book_frames_only() {
    // Server pushes one book frame followed by a last_trade_price frame. The orderbook
    // helper should yield exactly one item.
    let book_frame = r#"{
        "event_type": "book",
        "data": {
            "market": "0xmarket",
            "asset_id": "asset-1",
            "bids": [{"price": "0.4", "size": "10"}],
            "asks": [{"price": "0.6", "size": "20"}],
            "timestamp": 1700000000,
            "hash": "0xdead"
        }
    }"#
    .to_owned();
    let last_trade_frame = r#"{
        "event_type": "last_trade_price",
        "data": {
            "asset_id": "asset-1",
            "market": "0xmarket",
            "price": "0.5",
            "size": "1",
            "fee_rate_bps": "10",
            "side": "BUY",
            "timestamp": 1700000000,
            "transaction_hash": ""
        }
    }"#
    .to_owned();

    let (url, _) = spin_server(vec![book_frame, last_trade_frame]).await;
    let ws = build_ws_client(url, None);
    let mut stream = ws
        .subscribe_orderbook(vec!["asset-1".into()], MarketSubscribeOpts::default())
        .await
        .unwrap();

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("first event arrives")
        .expect("stream not closed")
        .expect("decoded ok");
    assert_eq!(first.asset_id, "asset-1");
    assert_eq!(first.bids[0].price, "0.4");

    // The last_trade_price frame should be filtered out — next() should pend.
    let next = tokio::time::timeout(Duration::from_millis(150), stream.next()).await;
    assert!(
        next.is_err(),
        "typed orderbook stream should not yield non-book frames; got: {next:?}"
    );
}

#[tokio::test]
async fn typed_midpoints_helper_computes_midpoint_from_book() {
    let book = r#"{
        "event_type": "book",
        "data": {
            "market": "0xm",
            "asset_id": "a",
            "bids": [{"price": "0.40", "size": "1"}],
            "asks": [{"price": "0.60", "size": "1"}],
            "timestamp": 1,
            "hash": ""
        }
    }"#
    .to_owned();
    let (url, _) = spin_server(vec![book]).await;
    let ws = build_ws_client(url, None);
    let mut stream = ws
        .subscribe_midpoints(vec!["a".into()], MarketSubscribeOpts::default())
        .await
        .unwrap();
    let mid = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(mid.midpoint.to_string(), "0.50");
}

#[tokio::test]
async fn typed_trades_helper_filters_user_channel_to_trades_only() {
    let order_frame = r#"{
        "event_type": "order",
        "data": {
            "id": "0xorder",
            "status": "ORDER_STATUS_LIVE",
            "asset_id": "1",
            "side": "BUY",
            "original_size": "10",
            "size_matched": "0",
            "price": "0.5",
            "type": "GTC"
        }
    }"#
    .to_owned();
    let trade_frame = r#"{
        "event_type": "trade",
        "data": {
            "id": "0xtrade",
            "status": "MATCHED",
            "asset_id": "1",
            "side": "BUY",
            "size": "5",
            "price": "0.09",
            "match_type": "MINT",
            "order_id": "0xorder"
        }
    }"#
    .to_owned();
    let (url, _) = spin_server(vec![order_frame, trade_frame]).await;

    let creds = Credentials::new(
        Uuid::nil(),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        "pass-1".into(),
    );
    let ws = build_ws_client(url, Some(creds));
    let mut stream = ws.subscribe_trades(vec!["0xcid".into()]).await.unwrap();
    let trade = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(trade.id, "0xtrade");
    assert_eq!(trade.match_type, "MINT");
}

#[tokio::test]
async fn subscribe_user_without_credentials_errors() {
    let client = Client::builder()
        .endpoints(
            Endpoints::new(
                "https://clob.example.com",
                "https://gamma.example.com",
                "ws://127.0.0.1:1/",
            )
            .unwrap(),
        )
        .build()
        .unwrap();
    let ws = client.clob_ws().unwrap();
    match ws.subscribe_user(vec![]).await {
        Ok(_) => panic!("expected credentials-not-attached error"),
        Err(e) => {
            assert!(
                e.to_string().contains("L2 credentials not attached"),
                "{e}"
            );
        }
    }
}
