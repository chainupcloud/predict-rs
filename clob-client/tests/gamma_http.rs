//! Locked-down `httpmock` tests asserting URL path and query-string assembly
//! for every `GammaClient` endpoint. These do not exercise the live server.

use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use predict_rs_clob_client::gamma::types::request::{
    CommentsByUserAddressRequest, GetSeriesRequest, ListCommentsRequest, ListCurationEventsRequest,
    ListEventCreatorsRequest, ListEventsRequest, ListSeriesRequest, ListTagsRequest,
    RelatedTagsRequest, SearchRequest,
};
use predict_rs_clob_client::gamma::GammaClient;
use reqwest::Client as HttpClient;
use url::Url;

fn client(server: &MockServer) -> GammaClient {
    let mut base = Url::parse(&server.base_url()).unwrap();
    base.set_path("/");
    GammaClient::new(HttpClient::new(), base)
}

#[tokio::test]
async fn health_hits_root_health_path() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body(serde_json::json!({
            "service": "gamma-api",
            "status": "ok",
            "timestamp": 1_700_000_000_000_i64,
        }));
    }).await;

    let out = client(&server).health().await.unwrap();
    mock.assert_async().await;
    assert_eq!(out.service, "gamma-api");
    assert_eq!(out.status, "ok");
}

#[tokio::test]
async fn list_events_serializes_filters_into_query_string() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET)
            .path("/events")
            .query_param("limit", "5")
            .query_param("offset", "10")
            .query_param("order", "created_at")
            .query_param("ascending", "true")
            .query_param("active", "true")
            .query_param("tag_id", "42");
        then.status(200).json_body(serde_json::json!([
            {"id": "1", "title": "Test event", "markets": [], "series": [], "tags": []}
        ]));
    }).await;

    let req = ListEventsRequest {
        limit: Some(5),
        offset: Some(10),
        order: Some("created_at".into()),
        ascending: Some(true),
        active: Some(true),
        tag_id: Some(42),
        ..Default::default()
    };
    let events = client(&server).list_events(&req).await.unwrap();
    mock.assert_async().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "1");
}

#[tokio::test]
async fn get_event_uses_path_parameter() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET).path("/events/123");
        then.status(200).json_body(serde_json::json!({
            "id": "123",
            "title": "By id",
            "markets": [],
            "series": [],
            "tags": [],
        }));
    }).await;

    let event = client(&server).get_event("123").await.unwrap();
    mock.assert_async().await;
    assert_eq!(event.id, "123");
}

#[tokio::test]
async fn get_event_by_slug_uses_slug_path() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET).path("/events/slug/world-cup-2026");
        then.status(200).json_body(serde_json::json!({
            "id": "5", "slug": "world-cup-2026", "markets": [], "series": [], "tags": [],
        }));
    }).await;

    let event = client(&server)
        .get_event_by_slug("world-cup-2026")
        .await
        .unwrap();
    mock.assert_async().await;
    assert_eq!(event.slug.as_deref(), Some("world-cup-2026"));
}

#[tokio::test]
async fn event_tags_endpoint() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET).path("/events/9/tags");
        then.status(200).json_body(serde_json::json!([
            {"id": "1", "label": "Politics"}
        ]));
    }).await;

    let tags = client(&server).event_tags("9").await.unwrap();
    mock.assert_async().await;
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].id, "1");
}

#[tokio::test]
async fn list_event_creators_passes_filters() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET)
            .path("/events/creators")
            .query_param("limit", "20")
            .query_param("creator_handle", "alice");
        then.status(200).json_body(serde_json::json!([]));
    }).await;

    let _ = client(&server)
        .list_event_creators(&ListEventCreatorsRequest {
            limit: Some(20),
            creator_handle: Some("alice".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn list_curation_events_passes_featured_level() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET)
            .path("/curation/events")
            .query_param("featured_level", "2");
        then.status(200).json_body(serde_json::json!([]));
    }).await;

    let _ = client(&server)
        .list_curation_events(&ListCurationEventsRequest {
            featured_level: Some(2),
        })
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn list_tags_passes_pagination() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET)
            .path("/tags")
            .query_param("limit", "100")
            .query_param("is_carousel", "true");
        then.status(200).json_body(serde_json::json!([
            {"id": "1", "label": "X", "slug": "x"}
        ]));
    }).await;

    let tags = client(&server)
        .list_tags(&ListTagsRequest {
            limit: Some(100),
            is_carousel: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    mock.assert_async().await;
    assert_eq!(tags.len(), 1);
}

#[tokio::test]
async fn get_tag_by_id_and_slug() {
    let server = MockServer::start_async().await;
    let m_id = server.mock_async(|when, then| {
        when.method(GET).path("/tags/7");
        then.status(200).json_body(serde_json::json!({"id": "7"}));
    }).await;
    let m_slug = server.mock_async(|when, then| {
        when.method(GET).path("/tags/slug/sports");
        then.status(200).json_body(serde_json::json!({"id": "9", "slug": "sports"}));
    }).await;

    let _ = client(&server).get_tag("7").await.unwrap();
    let _ = client(&server).get_tag_by_slug("sports").await.unwrap();
    m_id.assert_async().await;
    m_slug.assert_async().await;
}

#[tokio::test]
async fn related_tags_endpoints() {
    let server = MockServer::start_async().await;
    let m_id = server.mock_async(|when, then| {
        when.method(GET)
            .path("/tags/3/related-tags")
            .query_param("omit_empty", "true");
        then.status(200).json_body(serde_json::json!([]));
    }).await;
    let m_slug = server.mock_async(|when, then| {
        when.method(GET)
            .path("/tags/slug/sports/related-tags/tags")
            .query_param("status", "active");
        then.status(200).json_body(serde_json::json!([]));
    }).await;

    let req = RelatedTagsRequest {
        omit_empty: Some(true),
        ..Default::default()
    };
    let _ = client(&server).related_tags("3", &req).await.unwrap();
    let req = RelatedTagsRequest {
        status: Some("active".into()),
        ..Default::default()
    };
    let _ = client(&server)
        .tags_related_to_tag_by_slug("sports", &req)
        .await
        .unwrap();
    m_id.assert_async().await;
    m_slug.assert_async().await;
}

#[tokio::test]
async fn markets_endpoints() {
    let server = MockServer::start_async().await;
    let m_id = server.mock_async(|when, then| {
        when.method(GET)
            .path("/markets/55")
            .query_param("include_tag", "true");
        then.status(200).json_body(serde_json::json!({
            "id": "55",
            "conditionId": "0xabc",
            "clobTokenIds": "[\"123\",\"456\"]"
        }));
    }).await;
    let m_slug = server.mock_async(|when, then| {
        when.method(GET).path("/markets/slug/foo-bar");
        then.status(200).json_body(serde_json::json!({"id": "9", "conditionId": "0x00", "slug": "foo-bar"}));
    }).await;
    let m_tags = server.mock_async(|when, then| {
        when.method(GET).path("/markets/55/tags");
        then.status(200).json_body(serde_json::json!([]));
    }).await;
    let m_info = server.mock_async(|when, then| {
        when.method(POST)
            .path("/markets/information")
            .json_body(serde_json::json!({"clobTokenIds": ["1"]}));
        then.status(200).json_body(serde_json::json!([]));
    }).await;

    let market = client(&server).get_market("55", true).await.unwrap();
    assert_eq!(market.id, "55");
    assert_eq!(market.parsed_clob_token_ids(), vec!["123", "456"]);

    let _ = client(&server).get_market_by_slug("foo-bar", false).await.unwrap();
    let _ = client(&server).market_tags("55").await.unwrap();
    let _ = client(&server)
        .markets_information(&serde_json::json!({"clobTokenIds": ["1"]}))
        .await
        .unwrap();

    m_id.assert_async().await;
    m_slug.assert_async().await;
    m_tags.assert_async().await;
    m_info.assert_async().await;
}

#[tokio::test]
async fn series_endpoints() {
    let server = MockServer::start_async().await;
    let m_list = server.mock_async(|when, then| {
        when.method(GET).path("/series").query_param("limit", "3");
        then.status(200).json_body(serde_json::json!([]));
    }).await;
    let m_get = server.mock_async(|when, then| {
        when.method(GET)
            .path("/series/12")
            .query_param("exclude_events", "true");
        then.status(200).json_body(serde_json::json!({
            "id": "12", "events": [], "categories": [], "tags": [], "chats": [],
        }));
    }).await;
    let m_summary = server.mock_async(|when, then| {
        when.method(GET).path("/series-summary/12");
        then.status(200).json_body(serde_json::json!({
            "id": "12", "eventDates": [], "eventWeeks": [],
        }));
    }).await;
    let m_summary_slug = server.mock_async(|when, then| {
        when.method(GET).path("/series-summary/slug/world-cup");
        then.status(200).json_body(serde_json::json!({
            "id": "1", "eventDates": [], "eventWeeks": [],
        }));
    }).await;
    let m_count = server.mock_async(|when, then| {
        when.method(GET).path("/series/12/comments/count");
        then.status(200).json_body(serde_json::json!({"count": 9}));
    }).await;

    let _ = client(&server)
        .list_series(&ListSeriesRequest {
            limit: Some(3),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = client(&server)
        .get_series(
            "12",
            &GetSeriesRequest {
                exclude_events: Some(true),
                include_chat: None,
            },
        )
        .await
        .unwrap();
    let _ = client(&server).get_series_summary("12").await.unwrap();
    let _ = client(&server).get_series_summary_by_slug("world-cup").await.unwrap();
    let count = client(&server).series_comment_count("12").await.unwrap();
    assert_eq!(count.count, 9);
    m_list.assert_async().await;
    m_get.assert_async().await;
    m_summary.assert_async().await;
    m_summary_slug.assert_async().await;
    m_count.assert_async().await;
}

#[tokio::test]
async fn comment_endpoints() {
    let server = MockServer::start_async().await;
    let m_list = server.mock_async(|when, then| {
        when.method(GET)
            .path("/comments")
            .query_param("parent_entity_type", "Event")
            .query_param("parent_entity_id", "7");
        then.status(200).json_body(serde_json::json!([]));
    }).await;
    let m_get = server.mock_async(|when, then| {
        when.method(GET).path("/comments/1");
        then.status(200).json_body(serde_json::json!([{"id": "1"}]));
    }).await;
    let m_user = server.mock_async(|when, then| {
        when.method(GET)
            .path("/comments/user_address/0xaa")
            .query_param("limit", "5");
        then.status(200).json_body(serde_json::json!([]));
    }).await;

    let _ = client(&server)
        .list_comments(&ListCommentsRequest {
            parent_entity_type: Some("Event".into()),
            parent_entity_id: Some(7),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = client(&server).get_comment("1").await.unwrap();
    let _ = client(&server)
        .comments_by_user(
            "0xaa",
            &CommentsByUserAddressRequest {
                limit: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    m_list.assert_async().await;
    m_get.assert_async().await;
    m_user.assert_async().await;
}

#[tokio::test]
async fn profile_endpoints() {
    let server = MockServer::start_async().await;
    let m_pub = server.mock_async(|when, then| {
        when.method(GET)
            .path("/public-profile")
            .query_param("address", "0xabc");
        then.status(200).json_body(serde_json::json!({"name": "alice"}));
    }).await;
    let m_full = server.mock_async(|when, then| {
        when.method(GET).path("/profiles/user_address/0xabc");
        then.status(200).json_body(serde_json::json!({"id": "1", "name": "alice"}));
    }).await;

    let p = client(&server).get_public_profile("0xabc").await.unwrap();
    assert_eq!(p.name.as_deref(), Some("alice"));
    let full = client(&server)
        .get_profile_by_address("0xabc")
        .await
        .unwrap();
    assert_eq!(full.id, "1");
    m_pub.assert_async().await;
    m_full.assert_async().await;
}

#[tokio::test]
async fn search_endpoint() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(GET)
            .path("/public-search")
            .query_param("q", "world")
            .query_param("limit_per_type", "5");
        then.status(200).json_body(serde_json::json!({
            "events": [],
            "tags": [],
            "profiles": [],
            "pagination": {"hasMore": false, "totalResults": 0},
        }));
    }).await;

    let _ = client(&server)
        .search(&SearchRequest {
            q: "world".into(),
            limit_per_type: Some(5),
            ..Default::default()
        })
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn public_info_and_agreements_and_sport_types() {
    let server = MockServer::start_async().await;
    let m_pi = server.mock_async(|when, then| {
        when.method(GET).path("/public-info");
        then.status(200).json_body(serde_json::json!({
            "brand": {"title": "Demo", "logo": ""},
            "chain": {"chainId": 143},
            "contracts": {
                "exchangeAddress": "0x01",
                "negRiskExchangeAddress": "0x02",
                "ctfAddress": "0x03",
                "collateralToken": "0x04",
            },
            "loginStatement": "",
            "walletConnectProjectId": "",
        }));
    }).await;
    let m_ag = server.mock_async(|when, then| {
        when.method(GET).path("/agreements");
        then.status(200).json_body(serde_json::json!({"agreements": []}));
    }).await;
    let m_st = server.mock_async(|when, then| {
        when.method(GET).path("/config/sport-types");
        then.status(200).json_body(serde_json::json!({"types": []}));
    }).await;

    let pi = client(&server).public_info().await.unwrap();
    assert_eq!(pi.contracts.exchange_address, "0x01");
    let ag = client(&server).agreements().await.unwrap();
    assert!(ag.is_empty());
    let st = client(&server).sport_types().await.unwrap();
    assert!(st.types.is_empty());

    m_pi.assert_async().await;
    m_ag.assert_async().await;
    m_st.assert_async().await;
}

#[tokio::test]
async fn api_error_response_surfaces_status_code_and_body() {
    let server = MockServer::start_async().await;
    server.mock_async(|when, then| {
        when.method(GET).path("/tags/99999");
        then.status(404).json_body(serde_json::json!({
            "code": 40400, "message": "not found"
        }));
    }).await;

    let err = client(&server).get_tag("99999").await.unwrap_err();
    let s = err.to_string();
    assert!(s.contains("404"), "expected 404 in error: {s}");
    assert!(s.contains("tags/99999"), "expected path in error: {s}");
}

#[tokio::test]
async fn gamma_accessor_requires_endpoint() {
    // No gamma endpoint configured -> Client::gamma() returns Validation error.
    let client = predict_rs_clob_client::Client::new("https://clob-api.example.com").unwrap();
    assert!(client.gamma().is_err());
}
