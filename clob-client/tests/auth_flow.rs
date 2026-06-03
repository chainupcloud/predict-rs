//! End-to-end auth-flow integration tests using a mocked HTTP server.
//!
//! These tests exercise the [`Client`] L1 / L2 paths against a [`wiremock::MockServer`] and
//! assert that the on-wire headers match what the CLOB server expects to receive.
//!
//! The point is NOT to re-verify the cryptographic primitives (golden_signer.rs does that)
//! but to confirm:
//!
//! 1. The right URL path is hit for each method.
//! 2. The right HTTP method is used.
//! 3. All required `PRED_*` headers are present.
//! 4. The L2 HMAC matches the request that was actually sent (path-only, no query string).
//! 5. `create_or_derive_api_key` falls back to derive on HTTP error.

use predict_rs_clob_client::{
    AssetType, Client, Credentials, Endpoints, PMCup26Signer,
    auth::{build_l2_headers, compute_l2_hmac, current_timestamp, header},
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
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

fn assert_l1_headers(req: &Request) {
    assert_eq!(
        req.headers
            .get(header::PRED_ADDRESS)
            .expect("PRED_ADDRESS")
            .to_str()
            .unwrap()
            .to_lowercase(),
        EXPECTED_ADDR
    );
    assert!(req.headers.contains_key(header::PRED_SIGNATURE));
    assert!(req.headers.contains_key(header::PRED_TIMESTAMP));
    assert!(req.headers.contains_key(header::PRED_NONCE));
}

fn assert_l2_headers(req: &Request, expected_path: &str) {
    let api_key = req
        .headers
        .get(header::PRED_API_KEY)
        .expect("PRED_API_KEY")
        .to_str()
        .unwrap();
    assert_eq!(api_key, "11111111-2222-3333-4444-555555555555");
    let passphrase = req
        .headers
        .get(header::PRED_PASSPHRASE)
        .expect("PRED_PASSPHRASE")
        .to_str()
        .unwrap();
    assert_eq!(passphrase, TEST_PASSPHRASE);
    let ts = req
        .headers
        .get(header::PRED_TIMESTAMP)
        .expect("PRED_TIMESTAMP")
        .to_str()
        .unwrap();
    let sig = req
        .headers
        .get(header::PRED_SIGNATURE)
        .expect("PRED_SIGNATURE")
        .to_str()
        .unwrap();
    let method_str = req.method.as_str();
    let body_str = std::str::from_utf8(&req.body).unwrap_or_default();
    // The HMAC is computed over the PATH ONLY (no query string).
    let expected_sig = compute_l2_hmac(TEST_SECRET, ts, method_str, expected_path, body_str);
    assert_eq!(
        sig, expected_sig,
        "L2 signature mismatch for {method_str} {expected_path}: \
         on-wire body={body_str:?}, expected_sig={expected_sig}",
    );
    let addr = req
        .headers
        .get(header::PRED_ADDRESS)
        .expect("PRED_ADDRESS")
        .to_str()
        .unwrap()
        .to_lowercase();
    assert_eq!(addr, EXPECTED_ADDR);
}

async fn client_for(server: &MockServer, with_creds: bool) -> Client {
    let mut b = Client::builder()
        .endpoints(
            Endpoints::clob_only(server.uri()).expect("endpoint"),
        )
        .chain_id(CHAIN_ID);
    if with_creds {
        b = b.credentials(creds()).signer_address(signer().address());
    }
    b.build().unwrap()
}

#[tokio::test]
async fn create_api_key_sends_correct_l1_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiKey": "00000000-0000-0000-0000-000000000001",
            "secret": "secret-bytes",
            "passphrase": "pp-1",
        })))
        .mount(&server)
        .await;

    let client = client_for(&server, false).await;
    let got = client
        .create_api_key(&signer(), Some(7))
        .await
        .expect("create_api_key");
    assert_eq!(
        got.key,
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "POST");
    assert_eq!(requests[0].url.path(), "/auth/api-key");
    assert_eq!(
        requests[0]
            .headers
            .get(header::PRED_NONCE)
            .unwrap()
            .to_str()
            .unwrap(),
        "7"
    );
    assert_l1_headers(&requests[0]);
}

#[tokio::test]
async fn derive_api_key_uses_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/derive-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiKey": "00000000-0000-0000-0000-000000000002",
            "secret": "s2",
            "passphrase": "p2",
        })))
        .mount(&server)
        .await;

    let client = client_for(&server, false).await;
    let _ = client.derive_api_key(&signer(), None).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "GET");
    assert_eq!(requests[0].url.path(), "/auth/derive-api-key");
    assert_l1_headers(&requests[0]);
    // nonce defaults to "0" when None.
    assert_eq!(
        requests[0]
            .headers
            .get(header::PRED_NONCE)
            .unwrap()
            .to_str()
            .unwrap(),
        "0"
    );
}

#[tokio::test]
async fn delete_api_key_uses_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/auth/api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .mount(&server)
        .await;

    let client = client_for(&server, false).await;
    client
        .delete_api_key(&signer(), Uuid::nil())
        .await
        .expect("delete_api_key");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "DELETE");
    assert_l1_headers(&requests[0]);
    // The convenience wrapper hard-codes nonce = 0.
    assert_eq!(
        requests[0]
            .headers
            .get(header::PRED_NONCE)
            .unwrap()
            .to_str()
            .unwrap(),
        "0"
    );
}

#[tokio::test]
async fn delete_api_key_with_nonce_threads_nonce_through_header() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/auth/api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
        .mount(&server)
        .await;

    let client = client_for(&server, false).await;
    client
        .delete_api_key_with_nonce(&signer(), Uuid::nil(), 7)
        .await
        .expect("delete_api_key_with_nonce");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "DELETE");
    assert_l1_headers(&requests[0]);
    assert_eq!(
        requests[0]
            .headers
            .get(header::PRED_NONCE)
            .unwrap()
            .to_str()
            .unwrap(),
        "7"
    );
}

#[tokio::test]
async fn create_or_derive_falls_back_to_derive_on_conflict() {
    let server = MockServer::start().await;
    // Server returns 409 on create, then 200 on derive.
    Mock::given(method("POST"))
        .and(path("/auth/api-key"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error": "api key already exists"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/auth/derive-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiKey": "00000000-0000-0000-0000-0000000000aa",
            "secret": "derived",
            "passphrase": "derived-pp",
        })))
        .mount(&server)
        .await;

    let client = client_for(&server, false).await;
    let got = client
        .create_or_derive_api_key(&signer(), None)
        .await
        .expect("create_or_derive");
    assert_eq!(
        got.key,
        Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap()
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method.as_str(), "POST");
    assert_eq!(requests[1].method.as_str(), "GET");
}

#[tokio::test]
async fn create_or_derive_short_circuits_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiKey": "00000000-0000-0000-0000-0000000000bb",
            "secret": "fresh",
            "passphrase": "fresh-pp",
        })))
        .mount(&server)
        .await;

    let client = client_for(&server, false).await;
    let _ = client
        .create_or_derive_api_key(&signer(), None)
        .await
        .expect("create_or_derive");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "derive should not be called on 200");
}

#[tokio::test]
async fn api_keys_emits_l2_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/api-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "apiKeys": ["aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"],
            "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
            "proxy_wallet": "0x2100000000000000000000000000000000008946",
        })))
        .mount(&server)
        .await;

    let client = client_for(&server, true).await;
    let info = client.api_keys().await.unwrap();
    assert_eq!(info.api_keys.len(), 1);
    assert_eq!(
        info.proxy_wallet.as_deref(),
        Some("0x2100000000000000000000000000000000008946")
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_l2_headers(&requests[0], "/auth/api-keys");
}

#[tokio::test]
async fn balance_allowance_signs_path_only_not_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/balance-allowance"))
        .and(query_param("asset_type", "CONDITIONAL"))
        .and(query_param("token_id", "tok-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "balance": "12345",
            "allowances": { "0xabc": "99" },
        })))
        .mount(&server)
        .await;

    let client = client_for(&server, true).await;
    let resp = client
        .balance_allowance(AssetType::Conditional, Some("tok-1"))
        .await
        .unwrap();
    assert_eq!(resp.balance, "12345");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    // CRITICAL: HMAC is over "/balance-allowance" (path only), not
    // "/balance-allowance?asset_type=...".
    assert_l2_headers(&requests[0], "/balance-allowance");
}

#[tokio::test]
async fn balance_allowance_rejects_token_id_for_collateral() {
    let server = MockServer::start().await;
    let client = client_for(&server, true).await;
    let err = client
        .balance_allowance(AssetType::Collateral, Some("oops"))
        .await
        .expect_err("must reject token_id for collateral");
    assert!(matches!(err, predict_rs_clob_client::Error::Validation(_)));
    // No HTTP request should have been issued.
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn balance_allowance_requires_token_id_for_conditional() {
    let server = MockServer::start().await;
    let client = client_for(&server, true).await;
    let err = client
        .balance_allowance(AssetType::Conditional, None)
        .await
        .expect_err("must require token_id for conditional");
    assert!(matches!(err, predict_rs_clob_client::Error::Validation(_)));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn update_balance_allowance_hits_update_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/balance-allowance/update"))
        .and(query_param("asset_type", "COLLATERAL"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": "0"})))
        .mount(&server)
        .await;

    let client = client_for(&server, true).await;
    let _ = client
        .update_balance_allowance(AssetType::Collateral, None)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_l2_headers(&requests[0], "/balance-allowance/update");
}

#[tokio::test]
async fn l2_without_credentials_errors() {
    let server = MockServer::start().await;
    let client = client_for(&server, false).await;
    let err = client.api_keys().await.expect_err("no creds → error");
    assert!(matches!(err, predict_rs_clob_client::Error::NotAuthenticated));
}

#[tokio::test]
async fn l2_helper_round_trip_with_known_vector() {
    // Sanity-check: build_l2_headers + compute_l2_hmac produce the same value.
    let creds = creds();
    let addr = signer().address();
    let ts = current_timestamp();
    let headers = build_l2_headers(&creds, addr, &ts, "GET", "/auth/api-keys", "").unwrap();
    let expected = compute_l2_hmac(TEST_SECRET, &ts, "GET", "/auth/api-keys", "");
    assert_eq!(
        headers
            .get(header::PRED_SIGNATURE)
            .unwrap()
            .to_str()
            .unwrap(),
        expected
    );
}
