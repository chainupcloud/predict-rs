# Phase 3b — WebSocket subscriptions (market + user channels)

## Scope

Adds `pm-rs-clob-client::ws::*` (generic transport) and `pm-rs-clob-client::clob::ws::*`
(CLOB-specific client + types). Wires a `pm ws` subcommand tree into the CLI.

This phase delivers WebSocket parity with `pm-sdk-go/pkg/ws` and shape-parity
(not vendoring) with Polymarket V1's `rs-clob-client/src/{ws,clob/ws}/`.

## Public surface

### `pm_rs_clob_client::ws` (generic transport)

| Item | Kind | Notes |
|------|------|-------|
| `WsConfig` | struct | `ping_interval` / `initial_backoff` / `max_backoff` / `channel_capacity` / `connect_timeout` / `emit_reconnecting`. Defaults match the chainup server. |
| `WsConnection` | struct | Owns the reconnect loop + heartbeat task. Drop = abort. |
| `WsEvent` | enum | `Connected` / `Message(String)` / `Reconnecting{attempt, after}` / `Disconnected` / `Error(WsError)`. |
| `WsError` | enum | Variants: `Connect` / `Transport` / `Auth{status,message}` / `UserAuthRejected` / `Decode` / `Internal` / `Cancelled`. `is_fatal()` flags non-reconnectable cases. |

The `WsCommand` enum (`Send(String)` / `Close`) is `pub` under `ws::connection`
for advanced callers but is not re-exported at the crate root.

### `pm_rs_clob_client::clob::ws` (CLOB-specific)

| Item | Kind | Notes |
|------|------|-------|
| `ClobWebSocketClient` | struct | Constructed via `Client::clob_ws()`. `subscribe_market` / `subscribe_user` / `ping`. |
| `MarketSubscribeOpts` | struct | Builder: `with_initial_dump(bool)` / `with_level(MarketLevel)` / `with_custom_features(bool)`. |
| `MarketStream` | struct, implements `Stream<Item = Result<MarketEvent, WsError>>` | Methods: `subscribe(asset_ids)` / `unsubscribe(asset_ids)` / `close()`. |
| `UserStream` | struct, implements `Stream<Item = Result<UserEvent, WsError>>` | Methods: `subscribe(condition_ids)` / `unsubscribe(condition_ids)` / `close()`. |
| `MarketLevel` | enum (`One` / `Two` / `Three`) | Serializes to integer. |
| `MarketSubscribeRequest` / `MarketUpdateRequest` | struct | Wire envelopes; `Serialize` + `Deserialize`. |
| `UserSubscribeRequest` / `UserUpdateRequest` / `UserAuth` | struct | Wire envelopes. |
| `SubscriptionOperation` | enum (`Subscribe` / `Unsubscribe`) | |
| `MarketEvent` | enum | `Book` / `PriceChange` / `LastTradePrice` / `TickSizeChange` / `BestBidAsk` / `NewMarket` / `MarketResolved`. |
| `UserEvent` | enum | `Order(OrderEvent)` / `Trade(TradeEvent)`. |
| Event structs | struct | `BookEvent`, `PriceChangeEvent`, `PriceChangeEntry`, `LastTradePriceEvent`, `TickSizeChangeEvent`, `BestBidAskEvent`, `NewMarketEvent`, `MarketResolvedEvent`, `OrderEvent`, `TradeEvent`, `MakerOrderFill`, `OrderLevel`, `Timestamp`. |
| Status enums | enum | `OrderStatus` (6 variants), `TradeStatus` (5 variants), `OrderSubType`, `TraderSide`, `OrderSide`. |

`MarketLevel`, `MarketStream`, `UserStream`, `ClobWebSocketClient`, and
`MarketSubscribeOpts` are re-exported at the crate root for caller convenience.

### `pm_rs_clob_client::Client::clob_ws()`

Added one method that builds a `ClobWebSocketClient` bound to the parent's
WS endpoint + L2 credentials. Errors with `Error::Validation` if no WS
endpoint is configured.

## Subscription mechanics

- The market and user channels each dial a fresh socket. The base URL comes
  from `Endpoints::ws` (a `wss://` URL); the channel path (`ws/market` /
  `ws/user`) is appended via `url::Url::join`.
- Both channels send the subscribe envelope as the **first** WS frame after
  upgrade. The user channel carries `auth.apiKey` + `auth.passphrase` in
  that envelope; HTTP `PRED_*` headers are **not** used by the WS endpoints
  (this differs from the L2 REST contract — see
  `services/clob-service/internal/wsservice/user_channel.go`).
- Heartbeat: the client sends the text frame `"PING"` every 10 s. The server
  reply (text `"PONG"`) is consumed internally and never surfaced.
- Reconnect: exponential backoff with default cap 30 s. After every
  reconnect the SDK re-sends the most recent subscribe envelope so the
  server's view of the subscription matches the client's.
- Runtime `subscribe` / `unsubscribe` updates the local state and emits a
  `subscriptionRequestUpdate` envelope. Repeated subscribes are idempotent
  (the SDK tracks current asset / market IDs locally to avoid double-counting
  on reconnect).

## CLI subcommands

```
pm ws ping
pm ws book <ASSET_ID> [<ASSET_ID> …] [--no-initial-dump] [--level 1|2|3] [--custom-features] [--count N]
pm ws book-watch <ASSET_ID> [--print-as-json|--print-as-table]
pm ws user [--market <CONDITION_ID>]…
```

All subcommands honor Ctrl-C (graceful shutdown via `tokio::signal::ctrl_c`).

`pm ws user` auto-derives credentials via `GET /auth/derive-api-key` when
`--credentials` is not supplied, identically to the existing `pm balance` /
`pm auth list-keys` flows.

## Tests

| File | Coverage |
|------|----------|
| `clob-client/src/clob/ws/types/request.rs` (`#[cfg(test)]`) | Subscribe-envelope serialization (4 tests). |
| `clob-client/src/clob/ws/types/response.rs` (`#[cfg(test)]`) | Per-variant decode for every `event_type` (12 tests). |
| `clob-client/tests/ws_fixtures.rs` | Frozen-JSON round-trip per variant + per status enum (6 tests). |
| `clob-client/tests/ws_offline.rs` | End-to-end against a local `tokio_tungstenite` server (6 tests): subscribe-envelope + book decode, runtime sub/unsub, user-channel auth-in-first-frame, user-channel auth failure surfaces, ws-endpoint guard, credentials guard. |
| `clob-client/tests/ws_market_smoke.rs` | `#[ignore]`d live test against `wss://clob-ws.<tenant>/ws/market` — asserts ≥ 1 frame within 10 s. |

`cargo test --workspace` passes with 80+ tests (32 unit, 12 auth_flow, 18 gamma_http, 3 golden_signer, 6 ws_fixtures, 6 ws_offline, 2 doctests). The two `#[ignore]`d live tests (`gamma_smoke`, `ws_market_smoke`) are excluded from the default run.

## Known limitations

- The market stream surfaces every event variant verbatim. The Polymarket
  V1 SDK ships per-message filter streams (e.g. `subscribe_orderbook` returns
  only `BookUpdate` frames); the chainup SDK leaves that filtering to the
  caller for simplicity. Adding `subscribe_orderbook` / `subscribe_trades`
  helpers is a follow-on.
- `pm ws book-watch` renders one line per event. There is no in-place
  redrawing; `clearscreen` was considered but kept out to avoid a CLI dep.
- `MarketSubscribeOpts::initial_dump = Some(true)` is also sent on
  reconnect. The chainup server always returns a `book` snapshot per asset
  in that case, which is the desired behavior — the alternative would be to
  track "first connect vs reconnect" and risk an inconsistent local book.
- Reconnect does not retry on fatal auth errors (`WsError::Auth` /
  `WsError::UserAuthRejected`); both terminate the stream so the caller
  must explicitly re-build with corrected credentials.
- The `Timestamp` newtype accepts both numeric and quoted-string inputs but
  always serializes as a number on the way out. The Polymarket / chainup
  server output is always numeric, so round-trip parity holds in practice.

## Files added / modified

### Added

- `clob-client/src/ws/{mod,config,connection,error}.rs`
- `clob-client/src/clob/ws/{mod,client,subscription}.rs`
- `clob-client/src/clob/ws/types/{mod,request,response}.rs`
- `cli/src/ws_commands.rs`
- `clob-client/tests/{ws_fixtures,ws_offline,ws_market_smoke}.rs`
- `docs/ws.md`
- `PHASE3B_NOTES.md` (this file)

### Surgical edits

- `Cargo.toml` — added `tokio-tungstenite`, `futures-util`, `tokio-stream` workspace deps.
- `clob-client/Cargo.toml` — added `tokio`, `tokio-stream`, `tokio-tungstenite`, `futures`, `futures-util` to direct deps.
- `cli/Cargo.toml` — added `futures` + `futures-util` to direct deps.
- `clob-client/src/lib.rs` — added `pub mod ws;` + four crate-root re-exports.
- `clob-client/src/clob/mod.rs` — added `pub mod ws;`.
- `clob-client/src/client.rs` — added `Client::clob_ws()` at the bottom of the impl.
- `cli/src/cli.rs` — added `Command::Ws(WsArgs)` variant + four sub-arg structs.
- `cli/src/commands.rs` — added `Command::Ws` early-return arm + `resolve_endpoints_pub`.
- `cli/src/main.rs` — added `mod ws_commands;`.
- `docs/diff-vs-polymarket-v1.md` — appended five WS-row entries to §8.
