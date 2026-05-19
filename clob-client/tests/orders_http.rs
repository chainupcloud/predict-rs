//! Integration tests for Phase 2.2 order / trade endpoints.
//!
//! Uses `wiremock` to assert:
//!
//! - `POST /order` carries the L2 `PRED_*` headers and the expected JSON body shape.
//! - `POST /orders` rejects > 15 orders client-side.
//! - `DELETE /order` sends `{"orderID": "..."}`.
//! - `DELETE /orders` sends a bare JSON array.
//! - `DELETE /cancel-all` issues a body-less request.
//! - `GET /orders` includes `id` / `market` / `asset_id` / `status` / `next_cursor` in the
//!   query string and signs the path only.
//! - `GET /trades` injects the L2 signer address as `maker_address` and forwards `from_id`.
//! - `cancel_market_orders` validates that at least one of market / asset_id is set.

use pm_rs_clob_client::auth::{compute_l2_hmac, header};
use pm_rs_clob_client::{
    CancelMarketOrderRequest, Client, Credentials, Endpoints, OrdersRequest, PMCup26Signer,
    SignedOrder, TradesRequest,
};
use pm_rs_clob_client::clob::types::OrderType;
use pm_rs_clob_client::types::Side;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const EXPECTED_ADDR: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
const CHAIN_ID: u64 = 137;
const TEST_SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const TEST_PASSPHRASE: &str = "passphrase-test";

fn signer() -> PMCup26Signer {
    PMCup26Signer::from_hex(PRIVATE_KEY, CHAIN_ID).unwrap()
}

fn creds() -> Credentials {
    Credentials::new(
        Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
        TEST_SECRET.to_owned(),
        TEST_PASSPHRASE.to_owned(),
    )
}

async fn build_client(server: &MockServer) -> Client {
    Client::builder()
        .endpoints(Endpoints::clob_only(server.uri()).unwrap())
        .chain_id(CHAIN_ID)
        .credentials(creds())
        .signer_address(signer().address())
        .build()
        .unwrap()
}

/// Assert the request carries valid L2 headers and the HMAC matches *the path only* (i.e.
/// excludes the query string).
fn assert_l2_headers(req: &Request, expected_path: &str) {
    let ts = req.headers.get(header::PRED_TIMESTAMP).unwrap().to_str().unwrap();
    let sig = req.headers.get(header::PRED_SIGNATURE).unwrap().to_str().unwrap();
    let method_str = req.method.as_str();
    let body_str = std::str::from_utf8(&req.body).unwrap_or("");
    let expected_sig = compute_l2_hmac(TEST_SECRET, ts, method_str, expected_path, body_str);
    assert_eq!(
        sig, expected_sig,
        "L2 signature mismatch for {method_str} {expected_path}"
    );
    let addr = req
        .headers
        .get(header::PRED_ADDRESS)
        .unwrap()
        .to_str()
        .unwrap()
        .to_lowercase();
    assert_eq!(addr, EXPECTED_ADDR);
    assert_eq!(
        req.headers.get(header::PRED_API_KEY).unwrap().to_str().unwrap(),
        "11111111-2222-3333-4444-555555555555"
    );
    assert_eq!(
        req.headers.get(header::PRED_PASSPHRASE).unwrap().to_str().unwrap(),
        TEST_PASSPHRASE
    );
}

fn dummy_signed_order() -> SignedOrder {
    SignedOrder {
        salt: "12345".into(),
        maker: "0x000000000000000000000000000000000000dEaD".into(),
        signer: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into(),
        taker: "0x0000000000000000000000000000000000000000".into(),
        token_id: "100".into(),
        maker_amount: "34000000".into(),
        taker_amount: "100000000".into(),
        expiration: "0".into(),
        nonce: "0".into(),
        fee_rate_bps: "100".into(),
        side: Side::Buy,
        signature_type: "2".into(),
        signature: "0xabcd".into(),
        scope_id: "0x0000000000000000000000000000000000000000000000000000000000000001".into(),
    }
}

#[tokio::test]
async fn post_order_sends_l2_headers_and_correct_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/order"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "orderID": "snowflake-1",
            "status": "live",
            "takingAmount": "100000000",
            "makingAmount": "34000000",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let resp = client
        .post_order(dummy_signed_order(), OrderType::Gtc, false, "")
        .await
        .unwrap();
    assert_eq!(resp.order_id, "snowflake-1");
    assert_eq!(resp.status, "live");

    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/order");

    let body: Value = serde_json::from_slice(&received.body).unwrap();
    let order = &body["order"];
    // tokenID (camelCase) — server-side handlers.orderJSON uses mixed case.
    assert_eq!(order["tokenID"], "100");
    assert_eq!(order["makerAmount"], "34000000");
    assert_eq!(order["takerAmount"], "100000000");
    assert_eq!(order["feeRateBps"], "100");
    assert_eq!(order["side"], "BUY");
    assert_eq!(order["signatureType"], "2");
    assert!(order["signature"].as_str().unwrap().starts_with("0x"));
    assert_eq!(
        order["scopeId"],
        "0x0000000000000000000000000000000000000000000000000000000000000001"
    );
    assert_eq!(body["orderType"], "GTC");
    // postOnly / deferExec are omitted when false (skip_serializing_if).
    assert!(body.get("postOnly").is_none());
    assert!(body.get("deferExec").is_none());
}

#[tokio::test]
async fn post_orders_batch_serialises_as_array() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"success": true, "orderID": "a", "status": "live"},
            {"success": true, "orderID": "b", "status": "live"}
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let resp = client
        .post_orders(
            vec![dummy_signed_order(), dummy_signed_order()],
            OrderType::Gtc,
            false,
            "",
        )
        .await
        .unwrap();
    assert_eq!(resp.len(), 2);
    assert_eq!(resp[0].order_id, "a");
    assert_eq!(resp[1].order_id, "b");

    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/orders");
    let body: Value = serde_json::from_slice(&received.body).unwrap();
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn post_orders_rejects_more_than_15_client_side() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    let too_many: Vec<SignedOrder> = (0..16).map(|_| dummy_signed_order()).collect();
    let err = client
        .post_orders(too_many, OrderType::Gtc, false, "")
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("at most 15"), "unexpected error: {msg}");
}

#[tokio::test]
async fn cancel_order_sends_orderid_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/order"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "canceled": ["snowflake-1"],
            "not_canceled": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let resp = client.cancel_order("snowflake-1").await.unwrap();
    assert_eq!(resp.canceled, vec!["snowflake-1".to_string()]);
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/order");
    let body: Value = serde_json::from_slice(&received.body).unwrap();
    assert_eq!(body["orderID"], "snowflake-1");
}

#[tokio::test]
async fn cancel_orders_sends_bare_array_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "canceled": ["a", "b"],
            "not_canceled": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_client(&server).await;
    let _ = client
        .cancel_orders(&["a".into(), "b".into()])
        .await
        .unwrap();
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/orders");
    let body: Value = serde_json::from_slice(&received.body).unwrap();
    let arr = body.as_array().expect("body is a JSON array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0], "a");
    assert_eq!(arr[1], "b");
}

#[tokio::test]
async fn cancel_orders_rejects_more_than_3000_client_side() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    let too_many: Vec<String> = (0..3001).map(|i| i.to_string()).collect();
    let err = client.cancel_orders(&too_many).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("at most 3000"), "unexpected error: {msg}");
}

#[tokio::test]
async fn cancel_all_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/cancel-all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "canceled": ["a", "b"],
            "not_canceled": {}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let resp = client.cancel_all().await.unwrap();
    assert_eq!(resp.canceled.len(), 2);
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/cancel-all");
    assert!(received.body.is_empty());
}

#[tokio::test]
async fn cancel_market_orders_requires_market_or_asset() {
    let server = MockServer::start().await;
    let client = build_client(&server).await;
    let err = client
        .cancel_market_orders(CancelMarketOrderRequest::default())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("at least one of"), "unexpected: {msg}");
}

#[tokio::test]
async fn cancel_market_orders_sends_correct_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/cancel-market-orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "canceled": ["x"],
            "not_canceled": {}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let _ = client
        .cancel_market_orders(CancelMarketOrderRequest::by_asset("100"))
        .await
        .unwrap();
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/cancel-market-orders");
    let body: Value = serde_json::from_slice(&received.body).unwrap();
    assert_eq!(body["asset_id"], "100");
    assert!(body.get("market").is_none());
}

#[tokio::test]
async fn open_orders_paginated_query_and_path_only_hmac() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "limit": 100,
            "count": 1,
            "next_cursor": "LTE=",
            "data": [
                {
                    "id": "snowflake-1",
                    "status": "ORDER_STATUS_LIVE",
                    "owner": "owner-uuid",
                    "maker_address": "0xabc",
                    "market": "0xcondition",
                    "asset_id": "100",
                    "side": "BUY",
                    "outcome": "YES",
                    "original_size": "10",
                    "size_matched": "0",
                    "price": "0.34",
                    "expiration": "0",
                    "order_type": "GTC",
                    "created_at": "0",
                    "associate_trades": []
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let req = OrdersRequest {
        market: Some("0xcondition".into()),
        asset_id: Some("100".into()),
        ..Default::default()
    };
    let page = client.open_orders(&req, Some("LTE=")).await.unwrap();
    assert_eq!(page.count, 1);
    assert!(page.is_end());
    assert_eq!(page.data[0].id, "snowflake-1");
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/orders");
    let url = received.url.to_string();
    assert!(url.contains("market=0xcondition"), "missing market: {url}");
    assert!(url.contains("asset_id=100"), "missing asset_id: {url}");
    assert!(url.contains("next_cursor=LTE"), "missing cursor: {url}");
}

#[tokio::test]
async fn open_order_single_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/order/snowflake-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "snowflake-1",
            "status": "ORDER_STATUS_LIVE",
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let order = client.open_order("snowflake-1").await.unwrap();
    assert_eq!(order.id, "snowflake-1");
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/order/snowflake-1");
}

#[tokio::test]
async fn trades_fills_maker_address_from_signer() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/trades"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "limit": 100,
            "count": 0,
            "next_cursor": "LTE=",
            "data": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let req = TradesRequest {
        from_id: Some(42),
        limit: Some(50),
        ..Default::default()
    };
    let page = client.trades(&req, None).await.unwrap();
    assert!(page.is_end());
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/trades");
    let url = received.url.to_string();
    assert!(url.contains(&format!("maker_address={EXPECTED_ADDR}")), "missing maker_address: {url}");
    assert!(url.contains("from_id=42"), "missing from_id: {url}");
    assert!(url.contains("limit=50"), "missing limit: {url}");
}

#[tokio::test]
async fn order_scoring_single_lookup() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/order-scoring"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "scoring": true
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let resp = client.order_scoring("snowflake-1").await.unwrap();
    assert!(resp.scoring);
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/order-scoring");
    assert!(received.url.to_string().contains("order_id=snowflake-1"));
}

#[tokio::test]
async fn heartbeat_posts_empty_object() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/heartbeats"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = build_client(&server).await;
    let resp = client.heartbeat().await.unwrap();
    assert_eq!(resp.status, "ok");
    let received = &server.received_requests().await.unwrap()[0];
    assert_l2_headers(received, "/heartbeats");
    assert_eq!(&received.body, b"{}");
}
