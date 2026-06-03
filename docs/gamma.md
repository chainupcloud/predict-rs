# Gamma client

`predict-rs-clob-client` exposes the Gamma REST API via [`GammaClient`](../clob-client/src/gamma/client.rs).
Gamma is a separate REST service from CLOB; it lives at
`gamma-api.<tenant>` (e.g. `https://gamma-api.hermestrade.xyz`) and serves
market metadata: events, markets, tags, series, comments, profiles, search,
plus per-tenant curation and public-info catalogs. The SDK port mirrors
`pm-sdk-go/pkg/gamma` field-for-field against
`pm-cup2026/services/gamma-service/internal/models/models.go`.

Construct a sub-client from any [`Client`](../clob-client/src/client.rs) that
has a Gamma endpoint configured:

```rust
use predict_rs_clob_client::{Client, gamma::types::request::ListEventsRequest};

# async fn run() -> predict_rs_clob_client::Result<()> {
let client = Client::builder().tenant("hermestrade.xyz")?.build()?;
let gamma = client.gamma()?;
let events = gamma
    .list_events(&ListEventsRequest { limit: Some(5), ..Default::default() })
    .await?;
# let _ = events;
# Ok(()) }
```

If the client was constructed with only `--clob-endpoint` and no Gamma URL,
`Client::gamma()` returns `Error::Validation("gamma endpoint not configured: ...")`.

## Endpoint matrix

| HTTP | Path | SDK method | CLI subcommand | Status |
|------|------|------------|----------------|--------|
| GET  | `/health` | `health` | `predict-cli gamma health` | implemented |
| GET  | `/public-info` | `public_info` | `predict-cli gamma public-info` | implemented |
| GET  | `/agreements` | `agreements` | `predict-cli gamma agreements` | implemented |
| GET  | `/auth/jwks` | — | — | auth-owned (`jwt_login`) |
| GET  | `/auth/nonce` | — | — | auth-owned (`jwt_login`) |
| POST | `/auth/login` | — | — | auth-owned (`jwt_login`) |
| POST | `/auth/refresh` | — | — | auth-owned (`jwt_login`) |
| GET  | `/config/sport-types` | `sport_types` | `predict-cli gamma sport-types` | implemented |
| GET  | `/tags` | `list_tags` | `predict-cli gamma tags list` | implemented |
| GET  | `/tags/{id}` | `get_tag` | `predict-cli gamma tags get <id>` | implemented |
| GET  | `/tags/slug/{slug}` | `get_tag_by_slug` | `predict-cli gamma tags get <slug>` | implemented |
| GET  | `/tags/{id}/related-tags` | `related_tags` | `predict-cli gamma tags related <id>` | implemented |
| GET  | `/tags/{id}/related-tags/tags` | `tags_related_to_tag` | `predict-cli gamma tags related <id> --full` | implemented |
| GET  | `/tags/slug/{slug}/related-tags` | `related_tags_by_slug` | `predict-cli gamma tags related <slug>` | implemented |
| GET  | `/tags/slug/{slug}/related-tags/tags` | `tags_related_to_tag_by_slug` | `predict-cli gamma tags related <slug> --full` | implemented |
| GET  | `/events` | `list_events` | `predict-cli gamma events list` | implemented |
| GET  | `/events/{id}` | `get_event` | `predict-cli gamma events get <id>` | implemented |
| GET  | `/events/slug/{slug}` | `get_event_by_slug` | `predict-cli gamma events get <slug>` | implemented |
| GET  | `/events/{id}/tags` | `event_tags` | `predict-cli gamma events tags <id>` | implemented |
| GET  | `/events/creators` | `list_event_creators` | `predict-cli gamma events creators` | implemented |
| GET  | `/events/creators/{id}` | `get_event_creator` | `predict-cli gamma events creator <id>` | implemented |
| GET  | `/curation/events` | `list_curation_events` | `predict-cli gamma curation events` | implemented |
| GET  | `/markets/{id}` | `get_market` | `predict-cli gamma markets get <id>` | implemented |
| GET  | `/markets/slug/{slug}` | `get_market_by_slug` | `predict-cli gamma markets get <slug>` | implemented |
| GET  | `/markets/{id}/tags` | `market_tags` | `predict-cli gamma markets tags <id>` | implemented |
| POST | `/markets/information` | `markets_information` | `predict-cli gamma markets information --body '{...}'` | implemented |
| GET  | `/series` | `list_series` | `predict-cli gamma series list` | implemented |
| GET  | `/series/{id}` | `get_series` | `predict-cli gamma series get <id>` | implemented |
| GET  | `/series/{id}/comments/count` | `series_comment_count` | `predict-cli gamma series comments-count <id>` | implemented |
| GET  | `/series-summary/{id}` | `get_series_summary` | `predict-cli gamma series summary <id>` | implemented |
| GET  | `/series-summary/slug/{slug}` | `get_series_summary_by_slug` | `predict-cli gamma series summary <slug>` | implemented |
| GET  | `/comments` | `list_comments` | `predict-cli gamma comments list` | implemented |
| GET  | `/comments/{id}` | `get_comment` | `predict-cli gamma comments get <id>` | implemented |
| GET  | `/comments/user_address/{addr}` | `comments_by_user` | `predict-cli gamma comments by-user <addr>` | implemented |
| POST | `/profiles` | — | — | requires Bearer JWT |
| GET  | `/public-profile` | `get_public_profile` | `predict-cli gamma profiles public <addr>` | implemented |
| GET  | `/profiles/user_address/{addr}` | `get_profile_by_address` | `predict-cli gamma profiles get <addr>` | implemented |
| GET  | `/public-search` | `search` | `predict-cli gamma search <query>` | implemented |
| GET  | `/games`, `/games/{id}`, `/games/{id}/scores` | — | — | tenant-specific sports fixtures, tenant-specific, not implemented |
| GET  | `/sports-events` | — | — | aggregated fixture + market tree, tenant-specific, not implemented |
| POST | `/disputes/evidence` | — | — | write endpoint, not implemented |
| GET  | `/docs`, `/openapi.json` | — | — | server-side Scalar / spec, not relevant to SDK |

## Differences vs Polymarket Gamma

| Dimension | Polymarket Gamma | predict-rs Gamma |
|-----------|-----------------|-------------|
| Service URL | `https://gamma-api.polymarket.com` | `https://gamma-api.<tenant>` (multi-tenant) |
| Tenant isolation | none | hostname `Host` header → tenant ID + per-tenant rows |
| Wire stream | gamma SSE / streaming variant exists | REST only (no streaming in `gamma-service`) |
| `Event.titleTranslation` / `Market.questionTranslation` / `Market.outcomeTranslation` | absent | i18n payload (multi-language JSON string) |
| `Market.adjudication` | absent | UMA oracle lifecycle state + `nextSteps`, `questionId`, `adapterAddress` for the user-dapp dispute flow |
| `Market.sportPlayType` / `Market.adapterInstance` | absent | tenant-routing fields for the relayer |
| `/markets/information` body | exhaustive Polymarket filter | accepts a free-form JSON shape (`gamma-service` `MarketsInformationBody`); fields like `negRiskOther`, `rfqEnabled`, etc. do not exist here |
| `/curation/events` | absent | per-tenant featured / hero / highlight catalog |
| `/public-info` | absent | tenant brand + chain config + contract addresses |
| `/agreements` | absent | tenant agreements polling endpoint |
| `/config/sport-types` | absent | sport-type catalog from `kv_config[tag.types]` |
| `/sports` / `/sports/market-types` / `/teams` | present | not in `gamma-service` router; uses `/games*` and `/sports-events` instead |
| `/public-profile` shape | trader-statistics payload | returns a small profile block (no PnL — that lives in `data-service`) |
| `Comment.parentEntityType` values | mixed-case | `"Event"`, `"Market"`, `"Series"` (case-sensitive on the server — match exactly) |
| `Market.clob_token_ids` | JSON array of decimal strings | same JSON-array-string format; helper `Market::parsed_clob_token_ids` parses it |

## Constructing requests

Every list endpoint takes a `*Request` struct with `Option<T>` fields. Unset
fields are skipped during URL-encoding (`serde_html_form` + `skip_serializing_if`),
so the server falls back to its own defaults (`limit=20`, `offset=0`, etc.).

```rust
use predict_rs_clob_client::gamma::types::request::ListEventsRequest;

let req = ListEventsRequest {
    limit: Some(10),
    tag_id: Some(7),
    active: Some(true),
    ..Default::default()
};
```

`Vec<T>` fields (e.g. `SearchRequest::events_tag`) serialise as repeated keys
(`?events_tag=a&events_tag=b`) to match the chi handler's `parseStringSlice`
helper.

## Live smoke test

`tests/gamma_smoke.rs` calls `list_events(limit=2)` against
`https://gamma-api.hermestrade.xyz`. It is `#[ignore]` by default so
`cargo test --workspace` stays offline. To run it manually:

```bash
cargo test --workspace -- --ignored gamma_smoke
```

A locked-down `httpmock`-based suite (`tests/gamma_http.rs`) covers URL path
+ query-string assembly for every endpoint without touching the network.
