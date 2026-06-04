# predict-rs vs upstream V1 — differences

> **Scope:** `predict-rs` mirrors the upstream V1 toolchain (`rs-clob-client` v0.4 + the upstream CLI v0.1.4) functionally. This document lists **only the differences**; anything not listed here is unchanged or carried over directly.

Last updated: 2026-05-28.

---

## 1. Crate naming and distribution

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| SDK crate name | upstream V1 SDK crate | `predict-rs-clob-client` (hyphenated) |
| CLI crate name | upstream V1 CLI crate | `predict-cli` |
| CLI binary name | upstream CLI binary | `predict-cli` |
| Repository layout | Two independent git repos | Single Cargo workspace `predict-rs/{clob-client, cli}` |
| Config directory | upstream config dir (JSON) | `~/.config/predict/config.toml` — see [`docs/wallet.md`](wallet.md). Path overridable via `--config-dir`; file mode 0600, parent dir 0700, atomic-rename writes. |

---

## 2. Networks and collateral

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Default chain | Polygon (chainId 137) | Monad (chainId 143) — default `monad` network |
| Supported chains | Polygon + Amoy (hard-coded `phf::Map`) | Built-in `--network` registry in the CLI (Monad; more addable); the SDK itself stays chain-agnostic |
| Default REST endpoint | upstream hosted endpoint | `https://clob-api.hermestrade.xyz`; hostnames come from tenant config |
| Collateral | USDC.e (`0x2791…4174`) | USDC (contract address confirmed on-chain, injected via config) |
| Gas token | MATIC (Polygon) | Chain-dependent (OP Sepolia: ETH; Monad: MON) |
| Source of contract addresses | `phf_map!` in `lib.rs` (static table) | CLI's built-in network registry (`cli/src/networks/`) + CLI flags — **never in the SDK's `lib.rs`** |

> **Multi-tenant note:** the platform is a SaaS. Each tenant may have its own subdomain; multiple users under the same tenant share a unified order book via the same `scopeId`. The SDK holds no tenant or hostname state — the endpoint is supplied by the caller.

---

## 3. EIP-712 signing domains

### ClobAuth (L1 authentication)

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Domain name | `"ClobAuthDomain"` | `"ClobAuthDomain"` (same) |
| Domain version | `"1"` | `"1"` (same) |
| Domain `verifyingContract` | None (short-form) | None (same) |
| Field count | 4 | **5** |
| Field order | `address / timestamp / nonce / message` | `address / timestamp / nonce / `**`bytes32 scopeId`**` / message` |
| Type-hash string | `ClobAuth(address address,string timestamp,uint256 nonce,string message)` | `ClobAuth(address address,string timestamp,uint256 nonce,bytes32 scopeId,string message)` |

### Order (order signing)

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Domain name | upstream V1 domain string | **`"Prediction Market Protocol"`** |
| Domain version | `"1"` | `"1"` (same) |
| Domain `verifyingContract` | exchange contract | exchange contract (same) |
| Field count | 12 | **13** |
| Appended field | — | **`bytes32 scopeId`** |

> Upstream V2 moves `expiration` out of the signed domain and adds `timestamp / metadata / builder`; `predict-rs` does **not** follow V2 and keeps the V1 field layout plus `scopeId`.

---

## 4. Signature types

| Value | Upstream V1 | predict-rs |
|-------|---------------|-------|
| 0 | `Eoa` | `Eoa` (same) |
| 1 | `Proxy` (Magic / email) | `PolyProxy` (same semantics) |
| 2 | `GnosisSafe` (browser wallet) | `PolyGnosisSafe` (**default**, used by the Safe-wallet architecture) |
| 3 | — (V2 introduces `Poly1271`; V1 has none) | — |

> Platform users default to the Safe wallet flow: `maker = Safe address`, `signer = EOA`, `signatureType = 2`.

---

## 5. HTTP headers and encodings

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Auth-header prefix | `POLY_*` | **`PRED_*`** |
| L1: signature headers | `POLY_ADDRESS / POLY_NONCE / POLY_SIGNATURE / POLY_TIMESTAMP` | `PRED_ADDRESS / PRED_NONCE / PRED_SIGNATURE / PRED_TIMESTAMP` |
| L1: scope header | — | **`PRED_SCOPE_ID`** (optional, binds the new API key to a tenant scope) |
| L2: HMAC | `POLY_API_KEY / POLY_PASSPHRASE` + above | `PRED_API_KEY / PRED_PASSPHRASE` + above |
| HMAC secret encoding | base64 **URL-safe** | base64 **standard** |
| HMAC input string | `timestamp + method + path + body` | Same |
| L1 timestamp tolerance | ±5 min | ±5 min (same) |
| L2 timestamp tolerance | ±30 s | ±30 s (same) |
| Rate limit | 100 req/min sliding window (authenticated endpoints) | Same |

---

## 6. API endpoints

### Path / behavior differences

| Endpoint | Upstream V1 | predict-rs |
|----------|---------------|-------|
| `GET /time` | JSON object `{"time": ...}` | **Bare integer** body |
| `GET /midpoint` | `{"mid": ...}` | `{"price": ...}` (the response decoder accepts the legacy `mid` alias too) |
| `GET /price` | `{"price": ...}` | Same |
| `GET /balance-allowance` | EOA balance | Automatic **Safe-address derivation from EOA + scopeId**; returns Safe balance |
| `GET /auth/api-keys` | Lists API keys for the address | Also returns `proxy_wallet` (Safe address) |
| `GET /builder/trades` | Standard upstream V1 builder flow | `Client::builder_trades` exposes the L2-auth query path; the V1 `BuilderConfig::remote` remote-signer for `POST` paths is not implemented. |
| `POST /order` / `POST /orders` / `POST /orders/replace` | V1: built into `Client::post_order` / `post_orders` / `replace_order`. | Same surface (`Client::post_order` / `post_orders` / `replace_order`) — JSON shape matches `handlers.orderJSON` (camelCase `tokenID` / `makerAmount` / `feeRateBps` / `signatureType` / `scopeId`; `signatureType` as numeric **string**; salt as decimal string). |
| `DELETE /order` / `DELETE /orders` / `DELETE /cancel-all` / `DELETE /cancel-market-orders` | V1: `cancel_order` / `cancel_orders` / `cancel_all` / `cancel_market_orders`. | Same — `DELETE /orders` accepts both `["id"...]` (preferred) and `{"orderIDs": [...]}` per openapi; SDK sends the bare array form. |
| `GET /orders` / `GET /order/{id}` | V1: paginated `next_cursor` | Same envelope shape (`{limit, count, next_cursor, data}`); `next_cursor == "LTE="` signals end. Platform-specific `lazy` field surfaced in `OpenOrderResponse`. |
| `GET /trades` | V1: `before`/`after` filters | Adds `from_id` (snowflake ASC cursor) + `limit ∈ [1, 1000]`; SDK supports the full filter matrix and auto-fills `maker_address` from the configured L2 signer. |
| `POST /self-trade` | — | **Platform-only** (internal port `:8083`, used for market-maker price-history backfill / mirroring) |

### V1 endpoints `predict-rs` will not implement

| Module | Reason |
|--------|--------|
| `bridge` (cross-chain bridge) | the platform has no equivalent |
| `data` (on-chain data aggregator) | the platform uses its own subgraph, different protocol |
| `rtds` (real-time data stream) | upstream V1 proprietary |
| `rfq` (request for quote) | upstream V1 proprietary |
| `ctf` (EOA-broadcast split / merge / redeem) | Only Safe-mode writes are supported (`signatureType=2`). The CLI ships `predict-cli ctf split / merge / redeem` against the `relayer-service` (Safe meta-tx), but the EOA-direct broadcast variant upstream V1 ships is intentionally not provided. |
| `gamma` streaming | the platform's `gamma-service` is REST-only; no stream. REST surface is shipped — see [`docs/gamma.md`](gamma.md). |

### Upstream V1 CLOB endpoints not in `clob-service` (verified 2026-05-19)

Cross-checked against the platform repo's `services/clob-service/internal/tradingapi/server.go`. These will not be implemented unless the backend later adds them.

| Upstream V1 endpoint | Status | Notes |
|------------------------|--------|-------|
| `GET /markets` (paginated CLOB market list) | absent | Market discovery happens through Gamma (`/events`, `/markets`); CLOB exposes per-token reads only. |
| `GET /market/{condition_id}` | absent | Same — use Gamma `markets/{id}` or `markets/slug/{slug}` instead. |
| `GET /sampling-markets` / `/simplified-markets` / `/sampling-simplified-markets` | absent | Reward-program filters; no equivalent. |
| `GET /all-prices` | absent | No tenant-wide enumeration; callers should `POST /prices` with their token list. |
| `GET /neg-risk` (standalone) | absent (partial) | Neg-risk flag is returned **inside** the `/book` response, not exposed as its own endpoint. |
| `GET /geoblock` | absent | No geolocation middleware. |
| `GET /closed-only-mode` / `GET /account-status` | absent | No account-state introspection. |
| `GET|DELETE /notifications` | absent | No server-side notification queue. |
| `GET /rewards` / `GET /earnings/total/{date}` / `GET /earnings/markets/{date}` / `GET /reward-percentages` / `GET /current-rewards` / `GET /rewards/markets/{condition_id}` | absent | Upstream-affiliate maker-program endpoints; platform tenants run their own incentive logic. |
| `POST /orders-scoring` (batch) | absent | Singular `GET /order-scoring` is supported. |

### Upstream V1 endpoints with a different verb / shape

| Upstream V1 | predict-rs | Notes |
|---------------|---------|-------|
| `GET /midpoints` | `POST /midpoints` (also `GET`) | Batch midpoints. SDK call: `Client::midpoints(&[token_id])`. CLI: `predict-cli midpoints t1 t2 ...`. |
| `GET /prices` (batch) | `POST /prices` | SDK call: `Client::prices(&[(token, side)])`. CLI: `predict-cli prices t1:buy t2:sell ...`. |
| `GET /spreads` (batch) | `POST /spreads` | SDK call: `Client::spreads(&[token_id])`. CLI: `predict-cli spreads t1 t2 ...`. |
| `GET /books` (batch) | `POST /books` | SDK call: `Client::books(&[(token, side)])` → `Vec<Option<OrderBookSummary>>`. CLI: `predict-cli books t1:buy t2:sell ...`. |
| `GET /last-trades-prices` | `GET|POST /last-trades-prices` | Both verbs accepted; SDK sends POST. SDK call: `Client::last_trades_prices(&[token_id])` (capped at 500 client-side). CLI: `predict-cli last-trades t1 t2 ...`. |
| `GET /price-history?fidelity=...` | `GET /price-history?interval=...` | Supported intervals: `1H | 6H | 1D | 1W | 1M | ALL`. **No `1m` minute granularity.** SDK call: `Client::price_history(token_id, interval, fidelity, limit)`. CLI: `predict-cli price-history <token> --interval 1h`. |
| `POST /balance-allowance/update` | `GET /balance-allowance/update` | Verb difference; SDK already implements via `update_balance_allowance`. |

---

## 7. Order construction and fees

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Price precision | tick size 0.01 / 0.001 / 0.0001 | Same (the platform inherits the same rules) — enforced by `OrderBuilder::minimum_tick_size` |
| Token / USDC precision | 6 decimals | Same — `to_base_units` uses `Truncate(6).Shift(6)` matching `pm-sdk-go::toBaseUnits` |
| Order-field units | `size` / `amount` / `fee` human-readable, `makerAmount` / `takerAmount` in chain minimum units | Same |
| Fee-rate unit | bps (basis points); order field `feeRateBps` | Same |
| Fee algorithm | `fee = bps × amount` (flat) | **On-chain `min(p, 1−p)` adjusted formula** (see the platform repo's `services/clob-service/docs/fee-algorithm.md`) |
| Fee split | Single platform fee | **Split into `PLATFORM_FEE + TENANT_FEE`**, 3 on-chain transactions per side per fill |
| Salt generation | `(seconds × rand_f64) & (2^53 - 1)` | `time.Now().UnixNano() & (2^53 - 1)` (matches `pm-sdk-go::time.Now().UnixNano()`); pinned via `OrderBuilder::salt(...)` for reproducible signatures |
| `v` byte normalisation | n/a (V1 path emits {27,28} natively) | `+27` normalisation applied client-side (`normalize_ecdsa_v`); on-chain `ECDSA.recover` requires `{27, 28}`, server-side L2 verifier accepts both |
| `OrderBuilder` builder-codes / metadata / defer_exec | n/a in V1 | `deferExec` exists in the wire schema but the SDK always sends `false`; builder-program codes / metadata are V2-only and explicitly NOT carried |

---

## 8. SDK engineering structure

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Client state machine | Type-state pattern (`Client<Unauthenticated>` → `Client<Authenticated<K>>`) | **Single `Client` struct + `Option<Credentials>` + `Option<signer_address>`** — credentials and the L1 signer address are attached at build time via `ClientBuilder::credentials` / `signer_address` (no Builder / Normal / AWS-KMS Kind tiers) |
| Builder authentication | `promote_to_builder(BuilderConfig)` + remote signer service | Not implemented (no user requirement yet) |
| AWS KMS signer | Built-in `signer-aws` example | Not implemented (callers can plug in any `alloy::signers::Signer` impl) |
| Heartbeat long connection | `heartbeats` feature flag | Not implemented (`POST /heartbeats` reachable; long-poll variant deferred) |
| WebSocket asset-ID type | `Vec<String>` | Same — shipped (see [`docs/ws.md`](ws.md)) |
| WebSocket transport | `tokio_tungstenite`, type-state authenticated client | `tokio_tungstenite`, single-state client; auth carried in the first WS frame for the user channel (matches the server contract, **not** HTTP `PRED_*` headers) |
| WebSocket subscribe message | Single `SubscriptionRequest` covering both channels | Two distinct envelopes — `MarketSubscribeRequest` (`type=market`, `assets_ids`) and `UserSubscribeRequest` (`type=user`, `auth.{apiKey,passphrase}`, `markets`) |
| WebSocket heartbeat | Protocol-level Ping | Text frame `"PING"` / `"PONG"` (see `services/clob-service/internal/wsservice/`) |
| WebSocket runtime sub/unsub | Per-channel `subscribe` / `unsubscribe` envelopes | Same shape (`{"operation":"subscribe","assets_ids":[...]}` etc.) |
| WebSocket reconnect | Exponential backoff, re-emits subscriptions | Same |
| Public utility functions | Private | Not exposed; callers can compose `OrderBuilder` + signer + `Client` directly |

---

## 9. Error model

| Dimension | Upstream V1 | predict-rs |
|-----------|---------------|-------|
| Top-level error type | Layered enum with `downcast_ref::<Validation>()` | **Flat `thiserror`-based enum** |
| Variant matching | `match err.kind()` plus downcast | `match err` directly on the variant |

---

## 10. Future strategy regarding V2

`predict-rs` does **not** plan a wholesale migration to the upstream V2 SDK design (`OrderPayload` enum, `Poly1271`, dual-protocol auto-detection). If the platform's on-chain contracts ever adopt a V2-equivalent structure, this decision will be revisited.

V2 ideas worth borrowing **selectively** (not adopted wholesale):

- `build_sign_and_post()` one-shot order placement — partial: `OrderBuilder::build_and_sign` + `Client::post_order` are separate calls (caller chooses to compose them).
- `user_usdc_balance()` market-buy balance auto-adjustment — not implemented.
- A public `clob::utilities` module with the platform's fee formulas — not implemented.

---

## Appendix: confirming the signing baseline

Signing parity is the single biggest behavioral difference vs upstream V1 and the single biggest similarity vs `pm-sdk-go`. Byte-level parity is enforced by:

```bash
cargo test -p predict-rs-clob-client --test golden_signer
```

The fixtures (`clob-client/tests/fixtures/golden.json`) are a snapshot of `pm-sdk-go/pkg/signer/testdata/golden.json`. **Any signer change must clear this test before further work is layered on top.**
