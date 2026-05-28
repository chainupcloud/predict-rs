# pm-rs vs Polymarket V1 — differences

> **Scope:** `pm-rs` mirrors Polymarket V1 (`rs-clob-client` v0.4 + `polymarket-cli` v0.1.4) functionally. This document lists **only the differences**; anything not listed here is unchanged or carried over directly.

Last updated: 2026-05-28.

---

## 1. Crate naming and distribution

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| SDK crate name | `polymarket-client-sdk` (hyphenated) | `pm-rs-clob-client` (hyphenated) |
| CLI crate name | `polymarket-cli` | `pm-cli` |
| CLI binary name | `polymarket` | `pm` |
| Repository layout | Two independent git repos | Single Cargo workspace `pm-rs/{clob-client, cli}` |
| Config directory | `~/.config/polymarket/config.json` | `~/.config/pm/config.toml` — see [`docs/wallet.md`](wallet.md). Path overridable via `--config-dir` / `PM_CONFIG_DIR`; file mode 0600, parent dir 0700, atomic-rename writes. |

---

## 2. Networks and collateral

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Default chain | Polygon (chainId 137) | OP Sepolia (chainId 11155420) |
| Supported chains | Polygon + Amoy (hard-coded `phf::Map`) | **Any configurable EVM** (Monad / OP Sepolia / custom); hard-coding is forbidden |
| Default REST endpoint | `https://clob.polymarket.com` | `https://clob-api.hermestrade.xyz`; hostnames come from tenant config |
| Collateral | USDC.e (`0x2791…4174`) | USDC (contract address confirmed on-chain, injected via config) |
| Gas token | MATIC (Polygon) | Chain-dependent (OP Sepolia: ETH; Monad: MON) |
| Source of contract addresses | `phf_map!` in `lib.rs` (static table) | Runtime configuration (`Config` / CLI flag / env) — **not in `lib.rs`** |

> **Multi-tenant note:** `pm-cup2026` is a SaaS platform. Each tenant may have its own subdomain; multiple users under the same tenant share a unified order book via the same `scopeId`. The SDK holds no tenant or hostname state — the endpoint is supplied by the caller.

---

## 3. EIP-712 signing domains

### ClobAuth (L1 authentication)

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Domain name | `"ClobAuthDomain"` | `"ClobAuthDomain"` (same) |
| Domain version | `"1"` | `"1"` (same) |
| Domain `verifyingContract` | None (short-form) | None (same) |
| Field count | 4 | **5** |
| Field order | `address / timestamp / nonce / message` | `address / timestamp / nonce / `**`bytes32 scopeId`**` / message` |
| Type-hash string | `ClobAuth(address address,string timestamp,uint256 nonce,string message)` | `ClobAuth(address address,string timestamp,uint256 nonce,bytes32 scopeId,string message)` |

### Order (order signing)

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Domain name | `"Polymarket CTF Exchange"` | **`"Prediction Market Protocol"`** |
| Domain version | `"1"` | `"1"` (same) |
| Domain `verifyingContract` | exchange contract | exchange contract (same) |
| Field count | 12 | **13** |
| Appended field | — | **`bytes32 scopeId`** |

> Polymarket V2 moves `expiration` out of the signed domain and adds `timestamp / metadata / builder`; `pm-rs` does **not** follow V2 and keeps the V1 field layout plus `scopeId`.

---

## 4. Signature types

| Value | Polymarket V1 | pm-rs |
|-------|---------------|-------|
| 0 | `Eoa` | `Eoa` (same) |
| 1 | `Proxy` (Magic / email) | `PolyProxy` (same semantics) |
| 2 | `GnosisSafe` (browser wallet) | `PolyGnosisSafe` (**default**, used by the Safe-wallet architecture) |
| 3 | — (V2 introduces `Poly1271`; V1 has none) | — |

> `pm-cup2026` users default to the Safe wallet flow: `maker = Safe address`, `signer = EOA`, `signatureType = 2`.

---

## 5. HTTP headers and encodings

| Dimension | Polymarket V1 | pm-rs |
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

| Endpoint | Polymarket V1 | pm-rs |
|----------|---------------|-------|
| `GET /time` | JSON object `{"time": ...}` | **Bare integer** body |
| `GET /midpoint` | `{"mid": ...}` | `{"price": ...}` (the response decoder accepts the legacy `mid` alias too) |
| `GET /price` | `{"price": ...}` | Same |
| `GET /balance-allowance` | EOA balance | Automatic **Safe-address derivation from EOA + scopeId**; returns Safe balance |
| `GET /auth/api-keys` | Lists API keys for the address | Also returns `proxy_wallet` (Safe address) |
| `GET /builder/trades` | Standard Polymarket Builder flow | `Client::builder_trades` exposes the L2-auth query path; the V1 `BuilderConfig::remote` remote-signer for `POST` paths is not implemented. |
| `POST /order` / `POST /orders` / `POST /orders/replace` | V1: built into `Client::post_order` / `post_orders` / `replace_order`. | Same surface (`Client::post_order` / `post_orders` / `replace_order`) — JSON shape matches `handlers.orderJSON` (camelCase `tokenID` / `makerAmount` / `feeRateBps` / `signatureType` / `scopeId`; `signatureType` as numeric **string**; salt as decimal string). |
| `DELETE /order` / `DELETE /orders` / `DELETE /cancel-all` / `DELETE /cancel-market-orders` | V1: `cancel_order` / `cancel_orders` / `cancel_all` / `cancel_market_orders`. | Same — `DELETE /orders` accepts both `["id"...]` (preferred) and `{"orderIDs": [...]}` per openapi; SDK sends the bare array form. |
| `GET /orders` / `GET /order/{id}` | V1: paginated `next_cursor` | Same envelope shape (`{limit, count, next_cursor, data}`); `next_cursor == "LTE="` signals end. Platform-specific `lazy` field surfaced in `OpenOrderResponse`. |
| `GET /trades` | V1: `before`/`after` filters | Adds `from_id` (snowflake ASC cursor) + `limit ∈ [1, 1000]`; SDK supports the full filter matrix and auto-fills `maker_address` from the configured L2 signer. |
| `POST /self-trade` | — | **Platform-only** (internal port `:8083`, used for market-maker price-history backfill / mirroring) |

### V1 endpoints `pm-rs` will not implement

| Module | Reason |
|--------|--------|
| `bridge` (cross-chain bridge) | `pm-cup2026` has no equivalent |
| `data` (on-chain data aggregator) | `pm-cup2026` uses its own subgraph, different protocol |
| `rtds` (real-time data stream) | Polymarket proprietary |
| `rfq` (request for quote) | Polymarket proprietary |
| `ctf` (EOA-broadcast split / merge / redeem) | Only Safe-mode writes are supported (`signatureType=2`). The CLI ships `pm ctf split / merge / redeem` against the `relayer-service` (Safe meta-tx), but the EOA-direct broadcast variant Polymarket V1 ships is intentionally not provided. |
| `gamma` streaming | `pm-cup2026` `gamma-service` is REST-only; no stream. REST surface is shipped — see [`docs/gamma.md`](gamma.md). |

### Polymarket V1 CLOB endpoints not in `clob-service` (verified 2026-05-19)

Cross-checked against `pm-cup2026/services/clob-service/internal/tradingapi/server.go`. These will not be implemented unless the backend later adds them.

| Polymarket V1 endpoint | Status | Notes |
|------------------------|--------|-------|
| `GET /markets` (paginated CLOB market list) | absent | Market discovery happens through Gamma (`/events`, `/markets`); CLOB exposes per-token reads only. |
| `GET /market/{condition_id}` | absent | Same — use Gamma `markets/{id}` or `markets/slug/{slug}` instead. |
| `GET /sampling-markets` / `/simplified-markets` / `/sampling-simplified-markets` | absent | Reward-program filters; no equivalent. |
| `GET /all-prices` | absent | No tenant-wide enumeration; callers should `POST /prices` with their token list. |
| `GET /neg-risk` (standalone) | absent (partial) | Neg-risk flag is returned **inside** the `/book` response, not exposed as its own endpoint. |
| `GET /geoblock` | absent | No geolocation middleware. |
| `GET /closed-only-mode` / `GET /account-status` | absent | No account-state introspection. |
| `GET|DELETE /notifications` | absent | No server-side notification queue. |
| `GET /rewards` / `GET /earnings/total/{date}` / `GET /earnings/markets/{date}` / `GET /reward-percentages` / `GET /current-rewards` / `GET /rewards/markets/{condition_id}` | absent | Polymarket-affiliate maker-program endpoints; tenants on `pm-cup2026` run their own incentive logic. |
| `POST /orders-scoring` (batch) | absent | Singular `GET /order-scoring` is supported. |

### Polymarket V1 endpoints with a different verb / shape

| Polymarket V1 | pm-rs | Notes |
|---------------|---------|-------|
| `GET /midpoints` | `POST /midpoints` (also `GET`) | Batch midpoints. SDK call: `Client::midpoints(&[token_id])`. CLI: `pm midpoints t1 t2 ...`. |
| `GET /prices` (batch) | `POST /prices` | SDK call: `Client::prices(&[(token, side)])`. CLI: `pm prices t1:buy t2:sell ...`. |
| `GET /spreads` (batch) | `POST /spreads` | SDK call: `Client::spreads(&[token_id])`. CLI: `pm spreads t1 t2 ...`. |
| `GET /books` (batch) | `POST /books` | SDK call: `Client::books(&[(token, side)])` → `Vec<Option<OrderBookSummary>>`. CLI: `pm books t1:buy t2:sell ...`. |
| `GET /last-trades-prices` | `GET|POST /last-trades-prices` | Both verbs accepted; SDK sends POST. SDK call: `Client::last_trades_prices(&[token_id])` (capped at 500 client-side). CLI: `pm last-trades t1 t2 ...`. |
| `GET /price-history?fidelity=...` | `GET /price-history?interval=...` | Supported intervals: `1H | 6H | 1D | 1W | 1M | ALL`. **No `1m` minute granularity.** SDK call: `Client::price_history(token_id, interval, fidelity, limit)`. CLI: `pm price-history <token> --interval 1h`. |
| `POST /balance-allowance/update` | `GET /balance-allowance/update` | Verb difference; SDK already implements via `update_balance_allowance`. |

---

## 7. Order construction and fees

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Price precision | tick size 0.01 / 0.001 / 0.0001 | Same (`pm-cup2026` inherits the same rules) — enforced by `OrderBuilder::minimum_tick_size` |
| Token / USDC precision | 6 decimals | Same — `to_base_units` uses `Truncate(6).Shift(6)` matching `pm-sdk-go::toBaseUnits` |
| Order-field units | `size` / `amount` / `fee` human-readable, `makerAmount` / `takerAmount` in chain minimum units | Same |
| Fee-rate unit | bps (basis points); order field `feeRateBps` | Same |
| Fee algorithm | `fee = bps × amount` (flat) | **On-chain `min(p, 1−p)` adjusted formula** (see [`pm-cup2026/services/clob-service/docs/fee-algorithm.md`](../../pm-cup2026/services/clob-service/docs/fee-algorithm.md)) |
| Fee split | Single platform fee | **Split into `PLATFORM_FEE + TENANT_FEE`**, 3 on-chain transactions per side per fill |
| Salt generation | `(seconds × rand_f64) & (2^53 - 1)` | `time.Now().UnixNano() & (2^53 - 1)` (matches `pm-sdk-go::time.Now().UnixNano()`); pinned via `OrderBuilder::salt(...)` for reproducible signatures |
| `v` byte normalisation | n/a (V1 path emits {27,28} natively) | `+27` normalisation applied client-side (`normalize_ecdsa_v`); on-chain `ECDSA.recover` requires `{27, 28}`, server-side L2 verifier accepts both |
| `OrderBuilder` builder-codes / metadata / defer_exec | n/a in V1 | `deferExec` exists in the wire schema but the SDK always sends `false`; builder-program codes / metadata are V2-only and explicitly NOT carried |

---

## 8. SDK engineering structure

| Dimension | Polymarket V1 | pm-rs |
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

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Top-level error type | Layered enum with `downcast_ref::<Validation>()` | **Flat `thiserror`-based enum** |
| Variant matching | `match err.kind()` plus downcast | `match err` directly on the variant |

---

## 10. Future strategy regarding V2

`pm-rs` does **not** plan a wholesale migration to the Polymarket V2 SDK design (`OrderPayload` enum, `Poly1271`, dual-protocol auto-detection). If `pm-cup2026`'s on-chain contracts ever adopt a V2-equivalent structure, this decision will be revisited.

V2 ideas worth borrowing **selectively** (not adopted wholesale):

- `build_sign_and_post()` one-shot order placement — partial: `OrderBuilder::build_and_sign` + `Client::post_order` are separate calls (caller chooses to compose them).
- `user_usdc_balance()` market-buy balance auto-adjustment — not implemented.
- A public `clob::utilities` module with the platform's fee formulas — not implemented.

---

## Appendix: confirming the signing baseline

Signing parity is the single biggest behavioral difference vs Polymarket V1 and the single biggest similarity vs `pm-sdk-go`. Byte-level parity is enforced by:

```bash
cargo test -p pm-rs-clob-client --test golden_signer
```

The fixtures (`clob-client/tests/fixtures/golden.json`) are a snapshot of `pm-sdk-go/pkg/signer/testdata/golden.json`. **Any signer change must clear this test before further work is layered on top.**
