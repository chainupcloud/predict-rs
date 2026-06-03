//! Live-network smoke test hitting the `hermestrade.xyz` Gamma deployment.
//!
//! Marked `#[ignore]` so it never runs in the default `cargo test --workspace`
//! sweep. To execute:
//!
//! ```bash
//! cargo test --workspace -- --ignored gamma_smoke
//! ```
//!
//! The test:
//! 1. Builds a `Client` via `--tenant hermestrade.xyz`.
//! 2. Calls `GammaClient::list_events` with `limit = 2`.
//! 3. Asserts that the response decodes cleanly (zero or more `Event`s).
//!
//! It does NOT assert a specific non-empty list because the tenant may not yet
//! have published events. The point is to verify the wire format hasn't drifted.

use predict_rs_clob_client::gamma::types::request::ListEventsRequest;
use predict_rs_clob_client::Client;

#[tokio::test]
#[ignore = "live network — run with `cargo test --workspace -- --ignored gamma_smoke`"]
async fn gamma_smoke_list_markets_against_hermestrade() {
    let client = Client::builder()
        .tenant("hermestrade.xyz")
        .expect("tenant parse")
        .build()
        .expect("client build");
    let gamma = client.gamma().expect("gamma sub-client");

    let req = ListEventsRequest {
        limit: Some(2),
        ..Default::default()
    };
    let events = gamma.list_events(&req).await.expect("list_events");
    // Smoke test: decoding succeeded. Print a short summary so the operator can eyeball.
    println!("decoded {} events", events.len());
    for e in events {
        println!(
            "  id={} title={} markets={}",
            e.id,
            e.title.as_deref().unwrap_or(""),
            e.markets.len()
        );
    }
}
