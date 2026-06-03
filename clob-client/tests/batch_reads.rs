//! Integration tests for the batch read endpoints + price history.

use predict_rs_clob_client::clob::types::PriceHistoryInterval;
use predict_rs_clob_client::{Client, Endpoints, Side};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn build_client(server: &MockServer) -> Client {
    Client::builder()
        .endpoints(Endpoints::clob_only(server.uri()).unwrap())
        .build()
        .unwrap()
}

#[tokio::test]
async fn midpoints_posts_token_id_array_and_decodes_map() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/midpoints"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "t1": "0.5",
            "t2": "0.123"
        })))
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let out = client.midpoints(&["t1", "t2"]).await.expect("midpoints");

    assert_eq!(out.len(), 2);
    assert_eq!(out["t1"].to_string(), "0.5");
    assert_eq!(out["t2"].to_string(), "0.123");

    let req = server.received_requests().await.unwrap()[0].clone();
    assert_eq!(req.method.as_str(), "POST");
    let body: Vec<Value> = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0]["token_id"], "t1");
    assert_eq!(body[1]["token_id"], "t2");
}

#[tokio::test]
async fn prices_serializes_token_side_pairs_and_decodes_nested_map() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "t1": { "BUY": 0.51, "SELL": 0.52 },
            "t2": { "BUY": 0.10 }
        })))
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let out = client
        .prices(&[("t1".to_string(), Side::Buy), ("t2".to_string(), Side::Sell)])
        .await
        .expect("prices");
    assert_eq!(out["t1"]["BUY"], 0.51);
    assert_eq!(out["t1"]["SELL"], 0.52);

    let req = server.received_requests().await.unwrap()[0].clone();
    let body: Vec<Value> = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body[0]["side"], "BUY");
    assert_eq!(body[1]["side"], "SELL");
}

#[tokio::test]
async fn spreads_posts_and_decodes_map() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/spreads"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "t1": "0.02",
            "t2": "0"
        })))
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let out = client.spreads(&["t1", "t2"]).await.expect("spreads");
    assert_eq!(out["t1"].to_string(), "0.02");
    assert_eq!(out["t2"].to_string(), "0");
}

#[tokio::test]
async fn books_returns_one_slot_per_request_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/books"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "asset_id": "t1",
                "bids": [{"price": "0.4", "size": "10"}],
                "asks": [{"price": "0.6", "size": "10"}]
            },
            null
        ])))
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let out = client
        .books(&[("t1".to_string(), Side::Buy), ("t2".to_string(), Side::Sell)])
        .await
        .expect("books");
    assert_eq!(out.len(), 2);
    assert!(out[0].is_some());
    assert_eq!(out[0].as_ref().unwrap().asset_id, "t1");
    assert!(out[1].is_none());
}

#[tokio::test]
async fn last_trades_prices_decodes_array_and_caps_at_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/last-trades-prices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "token_id": "t1", "price": "0.5", "side": "BUY" },
            { "token_id": "t2", "price": "0.4", "side": "" }
        ])))
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let out = client
        .last_trades_prices(&["t1", "t2"])
        .await
        .expect("last_trades_prices");
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].token_id, "t1");
    assert_eq!(out[1].side, "");

    // Client-side cap.
    let too_many: Vec<&str> = (0..501).map(|_| "x").collect();
    let err = client.last_trades_prices(&too_many).await.unwrap_err();
    assert!(format!("{err}").contains("at most 500"));
}

#[tokio::test]
async fn price_history_passes_interval_and_decodes_points() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/price-history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "history": [
                { "t": 1_700_000_000, "p": "0.5" },
                { "t": 1_700_000_060, "p": "0.51" }
            ]
        })))
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let out = client
        .price_history("t1", PriceHistoryInterval::H1, Some(1), Some(100))
        .await
        .expect("price_history");
    assert_eq!(out.history.len(), 2);
    assert_eq!(out.history[0].t, 1_700_000_000);
    assert_eq!(out.history[1].p.to_string(), "0.51");

    let req = server.received_requests().await.unwrap()[0].clone();
    let q = req.url.query().unwrap_or("");
    assert!(q.contains("token_id=t1"));
    assert!(q.contains("interval=1H"));
    assert!(q.contains("fidelity=1"));
    assert!(q.contains("limit=100"));
}
