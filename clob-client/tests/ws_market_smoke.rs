//! Live network smoke test against the `clob-ws` host.
//!
//! Marked `#[ignore]`; run with:
//!
//! ```bash
//! cargo test --workspace -- --ignored ws_market_smoke
//! ```
//!
//! Configurable via env vars (defaults target the hermestrade dev host):
//!
//! - `PM_WS_TENANT` — tenant host (default `hermestrade.xyz`).
//! - `PM_WS_ASSET_IDS` — comma-separated list of asset (token) IDs. If unset
//!   we use two well-known asset IDs from the dev environment. The test only
//!   asserts that at least one frame arrives, so it is robust to those IDs
//!   becoming inactive — the connection itself is what's exercised.

use std::time::Duration;

use futures::StreamExt as _;
use predict_rs_clob_client::{Client, MarketSubscribeOpts};

#[tokio::test]
#[ignore = "live network — run with `cargo test --workspace -- --ignored ws_market_smoke`"]
async fn ws_market_smoke_at_least_one_frame() {
    let tenant = std::env::var("PM_WS_TENANT").unwrap_or_else(|_| "hermestrade.xyz".into());
    let raw_assets = std::env::var("PM_WS_ASSET_IDS").unwrap_or_else(|_| {
        // Two arbitrary ids from the dev gamma; the server is fine with
        // unrecognised IDs (it just won't push frames for them). We pair
        // with a public-info derived id (if available) below.
        "84121906415324290604740611420149014066823478272700175614376523125253089033054,\
12345"
            .into()
    });
    let asset_ids: Vec<String> = raw_assets
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    assert!(!asset_ids.is_empty(), "need at least one PM_WS_ASSET_IDS entry");

    let client = Client::builder()
        .tenant(&tenant)
        .unwrap()
        .build()
        .unwrap();
    let ws = client.clob_ws().expect("ws endpoint resolved from tenant");

    let mut stream = ws
        .subscribe_market(
            asset_ids,
            MarketSubscribeOpts::default().with_initial_dump(true),
        )
        .await
        .expect("subscribe");

    let next = tokio::time::timeout(Duration::from_secs(10), stream.next()).await;
    match next {
        Ok(Some(Ok(_event))) => {}
        Ok(Some(Err(e))) => panic!("ws error: {e}"),
        Ok(None) => panic!("stream closed unexpectedly"),
        Err(_) => panic!("timeout after 10s — no frame from live server"),
    }
}
