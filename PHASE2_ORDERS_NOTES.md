# Phase 2.2 — order construction, signing, placement, queries

Branch: `feat/phase2-orders`. Builds on Phase 2.1 (L1 / L2 auth + balance-allowance). All
tests are green (`cargo test --workspace`), `cargo build --release --workspace` succeeds.

## Endpoints implemented

| Method | Path | Auth | `Client` method | CLI |
|--------|------|------|-----------------|-----|
| POST | `/order` | L2 | `post_order` | `pm order create` |
| POST | `/orders` | L2 | `post_orders` | (composed by `pm order create`) |
| POST | `/orders/replace` | L2 | `replace_order` | `pm order replace` |
| DELETE | `/order` | L2 | `cancel_order` | `pm order cancel` |
| DELETE | `/orders` | L2 | `cancel_orders` | `pm order cancel-many` |
| DELETE | `/cancel-all` | L2 | `cancel_all` | `pm order cancel-all` |
| DELETE | `/cancel-market-orders` | L2 | `cancel_market_orders` | `pm order cancel-market` |
| GET | `/orders` | L2 | `open_orders` | `pm order list` |
| GET | `/order/{orderID}` | L2 | `open_order` | `pm order get` |
| GET | `/trades` | L2 | `trades` | `pm trade` |
| GET | `/builder/trades` | L2 | `builder_trades` | `pm trade --builder` |
| GET | `/order-scoring` | L2 | `order_scoring` / `orders_scoring` | `pm order scoring` |
| POST | `/heartbeats` | L2 | `heartbeat` | `pm heartbeat` |

## New public types (re-exported at the crate root)

- `pm_rs_clob_client::OrderBuilder<K>` with `K = Limit | Market`.
- `pm_rs_clob_client::OrderType` (`Gtc` / `Gtd` / `Fok` / `Fak`) — wire form is bare `"GTC"` / `"GTD"` / `"FOK"` / `"FAK"`.
- `pm_rs_clob_client::Side` and `pm_rs_clob_client::SignatureType` re-exported from the existing `types` module for ergonomics.
- `pm_rs_clob_client::SignableOrder` — typed in-memory companion to `SignedOrder` (carries `order_type` / `post_only` / `owner`).
- `pm_rs_clob_client::SignedOrder` — wire-ready JSON shape (camelCase `tokenID` / `makerAmount` / `takerAmount` / `feeRateBps` / `signatureType` / `scopeId`). Serialises identically to `services/clob-service/internal/tradingapi/handlers.orderJSON`.
- `pm_rs_clob_client::SendOrderRequest` — outer envelope for `POST /order` (and per-item batch).
- `pm_rs_clob_client::PostOrderResponse` / `CancelOrdersResponse` / `CancelMarketOrderRequest` / `OpenOrderResponse` / `TradeResponse` / `OrderScoringResponse` / `HeartbeatResponse`.
- `pm_rs_clob_client::ReplaceOrdersRequest` / `ReplaceOrdersResponse` (with per-cancel `ReplaceCancelResult` and per-place `ReplacePlaceResult`).
- `pm_rs_clob_client::OrdersRequest` / `TradesRequest` — typed builders for the query-string parameters.
- `pm_rs_clob_client::Page<T>` — generic cursor envelope `{limit, count, next_cursor, data}`. `Page::END_CURSOR = "LTE="`; `Page::is_end()` accepts both `"LTE="` and the empty string.

## New helpers in `clob::order_builder`

- `OrderBuilder::limit()` / `OrderBuilder::market()` — entry points.
- Limit builder: `.token_id() / .price() / .size() / .side() / .order_type() / .post_only() / .expiration() / .nonce() / .fee_rate_bps() / .maker() / .taker() / .signature_type() / .salt() / .owner() / .minimum_tick_size()`. Build with `.build()` (returns `SignableOrder`) or `.build_and_sign(&signer)` (returns `(SignableOrder, SignedOrder)`).
- Market builder: same as limit, plus `.shares(...)` / `.usdc(...)` to specify the market amount in shares (default) or USDC (BUY only).
- `signed_order_from(&signable, &sig_65)` — escape hatch for callers that want to plug in an external signer (e.g. AWS-KMS).
- `normalize_ecdsa_v(sig_65) -> sig_65` — `v ∈ {0,1} → v ∈ {27,28}` per `pm-sdk-go::normalizeECDSAv`.
- `compute_amounts(side, price, size)` — public-ish (`pub(crate)`) primitive used by the builder.

## Wire-level decisions and the evidence behind them

| Decision | Evidence |
|----------|----------|
| `SignedOrder` JSON uses camelCase field names (`tokenID`, `makerAmount`, etc.) | `services/clob-service/internal/tradingapi/handlers/order.go::orderJSON` — verbatim. Cross-checked in `tests/orders_sign_roundtrip.rs::signed_order_json_shape` (the test enumerates every expected JSON key). |
| `signatureType` is a JSON **string** (`"0"` / `"1"` / `"2"`) | openapi.yaml line 1533–1535 declares `enum: ["0", "1", "2"]`, and `parseOrderRequest` uses `strconv.ParseUint(req.Order.SignatureType, 10, 32)`. |
| `side` is `"BUY"` / `"SELL"` | openapi `Side` enum + the `strings.ToUpper(req.Order.Side)` switch in `parseOrderRequest`. |
| `salt` is a decimal-string `*big.Int` | `parseOrderRequest`'s `new(big.Int).SetString(req.Order.Salt, 10)`. The SDK emits `U256::to_string()` (base-10). |
| `scopeId` is omitted when zero | The L1 header path omits when zero; we mirror that for the order body's `scopeId` field via `skip_serializing_if = "String::is_empty"`. The Go handler treats empty as "no scope" — `parseOrderRequest` only copies the bytes when `req.Order.ScopeID != ""`. |
| `v ∈ {27, 28}` on the wire | `pm-sdk-go/pkg/clob/helpers.go::normalizeECDSAv` adds 27. The server-side `VerifyOrderSignature` (`crypto/order_eip712.go`) accepts both by subtracting 27 if `≥ 27`; on-chain settlement (`CTFExchange.matchOrders → OZ ECDSA.recover`) demands `{27, 28}`. The SDK + pm-sdk-go agree on `{27, 28}` end-to-end. |
| `DELETE /orders` body is a bare JSON array | `handlers.CancelOrders` accepts both `["id"...]` and `{"orderIDs": [...]}` (try-array-first), but openapi defines the array form first. SDK sends the array. Asserted in `tests/orders_http.rs::cancel_orders_sends_bare_array_body`. |
| `from_id` is a snowflake ASC cursor | openapi line 1006-1015. The Go handler clamps `limit ∈ [1, 1000]` and defaults to legacy DESC ordering when `from_id` is absent. SDK forwards all three (`before` / `after` / `from_id`). |
| HMAC over path only (no query string) | Carried over from Phase 2.1 — every L2 method uses `Client::request_authenticated` which signs `path` only. Asserted by `tests/orders_http.rs::open_orders_paginated_query_and_path_only_hmac`. |
| `next_cursor: "LTE="` = end of stream | openapi line 743-746 + `handlers/query.go::endCursor = "LTE="`. `Page::is_end()` accepts both `"LTE="` and the empty string for resilience. |
| `trades.maker_address` is server-required | openapi parameter required:true. SDK auto-fills from configured signer when `TradesRequest::maker_address` is `None`. |

## Tests

`cargo test --workspace` runs 88 tests (all green, plus 2 ignored live-network smokes):

- `clob-client` unit (34): existing signer/types/endpoints/auth + 13 new builder math + tick + side + v-norm tests in `clob::order_builder::tests`.
- `tests/golden_signer.rs` (3) — Phase 1's golden vectors, **unchanged and still passing**.
- `tests/auth_flow.rs` (12) — Phase 2.1 L1 / L2 auth wire tests, **unchanged and still passing**.
- `tests/orders_http.rs` (14) — new: wiremock assertions for every Phase 2.2 endpoint. Notable:
  - `post_order_sends_l2_headers_and_correct_envelope` — body shape parity with `handlers.orderJSON`.
  - `post_orders_batch_serialises_as_array`, `cancel_orders_sends_bare_array_body`.
  - `open_orders_paginated_query_and_path_only_hmac` — proves HMAC is over path only even when a query string is present.
  - `trades_fills_maker_address_from_signer` — auto-fill of `maker_address`.
  - Limit checks: `>15` orders, `>3000` cancel ids rejected client-side.
- `tests/orders_sign_roundtrip.rs` (7) — new: golden-vector round trip, JSON shape enumeration, scope-omitted-when-zero, builder-vs-formula parity, `+27` v normalisation.

## Known limitations / handoff questions

1. **No client-side Safe-address derivation.** When `signatureType = PolyGnosisSafe` the SDK requires the caller to pass `.maker(<Safe address>)` explicitly. Phase 3+ should add `signer::derive_safe_address(eoa, scope_id, factory, master_copy, proxy_creation_code)` — outlined in `clob-service/CLAUDE.md` "Safe 钱包架构".
2. **`OrderBuilder::market` does not walk the book client-side.** Polymarket V1's `MarketBuilder::calculate_price` walks asks/bids to find the cutoff price; chainup runs the actual book-walk server-side, so the client signs at an anchor price. Callers that want the V1-style depth check should call `Client::book(...)` themselves.
3. **`pm order replace --orders-file` accepts pre-signed envelopes only.** The CLI does not currently mint+sign N new orders inline for the replace path. Use `pm order create --dry-run` (which prints the full `SendOrder` envelope JSON) per new order, then concatenate into the file. SDK callers can build the `ReplaceOrdersRequest` programmatically.
4. **Phase 2.1's `delete_api_key` nonce=0 hard-code is unchanged.** Tracked in `PHASE2_NOTES.md`; not affected by Phase 2.2.
5. **No live smoke test** included. Live verification against the dev tenant requires real `PM_PRIVATE_KEY` + `PM_EXCHANGE_ADDRESS` + a known maker Safe address; ignored-by-default tests would just race other developers on shared state.
6. **`order-scoring` batch is a fan-out, not a single endpoint.** Server has no batch `/order-scoring`; `Client::orders_scoring` issues N requests sequentially.
7. **Builder POST endpoints (with `PRED_BUILDER_*` headers) are out of scope.** `GET /builder/trades` is exposed (since it's just a read), but creating a builder API key / signing as a builder is deferred.

## Files changed / added

```
clob-client/Cargo.toml                          + rand dep, rust_decimal_macros dev-dep
clob-client/src/clob/types.rs                   ≈+330 lines: OrderType, SignedOrder, SignableOrder, SendOrderRequest, PostOrderResponse, CancelOrdersResponse, CancelMarketOrderRequest, OpenOrderResponse, OrdersRequest, TradeResponse, TradesRequest, OrderScoringResponse, HeartbeatResponse, ReplaceOrdersRequest, ReplaceOrdersResponse, ReplaceCancelResult, ReplacePlaceResult, Page<T>
clob-client/src/clob/order_builder.rs           NEW, ≈700 lines: OrderBuilder<Limit|Market>, build / build_and_sign, compute_amounts, normalize_ecdsa_v, signed_order_from, validate_*, plus 18 unit tests
clob-client/src/clob/mod.rs                     + pub mod order_builder
clob-client/src/lib.rs                          + 12 re-exports
clob-client/src/client.rs                       ≈+260 lines: 14 new Client methods (limit_order, market_order, post_order, post_orders, replace_order, cancel_order, cancel_orders, cancel_all, cancel_market_orders, open_orders, open_order, trades, builder_trades, order_scoring, orders_scoring, heartbeat)
clob-client/tests/orders_http.rs                NEW, 14 wiremock integration tests
clob-client/tests/orders_sign_roundtrip.rs      NEW, 7 round-trip tests against golden vectors
cli/src/cli.rs                                  + Order(OrderCommand) / Trade(TradeArgs) / Heartbeat variants
cli/src/main.rs                                 + mod order_commands
cli/src/commands.rs                             + 3 dispatch arms; visibility bump on shared helpers
cli/src/order_commands.rs                       NEW, ≈590 lines: pm order {create, cancel, cancel-many, cancel-market, cancel-all, list, get, replace, scoring} + pm trade + pm heartbeat
docs/orders.md                                  NEW, Phase 2.2 lifecycle / fee / SDK + CLI examples
docs/diff-vs-polymarket-v1.md                   updated Phase 2.2 deltas
PHASE2_ORDERS_NOTES.md                          this file
```
