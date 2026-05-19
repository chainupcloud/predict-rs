# pm-rs vs Polymarket V1 — differences

> **Scope:** `pm-rs` mirrors Polymarket V1 (`rs-clob-client` v0.4 + `polymarket-cli` v0.1.4) functionally. This document lists **only the differences**; anything not listed here is unchanged or carried over directly.

Last updated: 2026-05-19 (Phase 2.1: L1/L2 auth + balance-allowance).

---

## 1. Crate naming and distribution

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| SDK crate name | `polymarket-client-sdk` (hyphenated) | `pm-rs-clob-client` (hyphenated) |
| CLI crate name | `polymarket-cli` | `pm-cli` |
| CLI binary name | `polymarket` | `pm` |
| Repository layout | Two independent git repos | Single Cargo workspace `pm-rs/{clob-client, cli}` |
| Config directory | `~/.config/polymarket/config.json` | `~/.config/pm/config.toml` (planned for Phase 2) |

---

## 2. Networks and collateral

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Default chain | Polygon (chainId 137) | OP Sepolia (chainId 11155420) |
| Supported chains | Polygon + Amoy (hard-coded `phf::Map`) | **Any configurable EVM** (Monad / OP Sepolia / custom); hard-coding is forbidden |
| Default REST endpoint | `https://clob.polymarket.com` | `https://clob-api.predict.prax1s.xyz` (dev); production hostnames come from tenant config |
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
| 2 | `GnosisSafe` (browser wallet) | `PolyGnosisSafe` (**chainup default**, used by the Safe-wallet architecture) |
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
| `GET /builder/trades` | Standard Polymarket Builder flow | Same endpoint; Phase 2 implements only the L2 part (the V1 `BuilderConfig::remote` remote signer is out of scope) |
| `POST /self-trade` | — | **chainup-only** (internal port `:8083`, used for market-maker price-history backfill / mirroring) |

### V1 endpoints `pm-rs` will not implement

| Module | Reason |
|--------|--------|
| `bridge` (cross-chain bridge) | `pm-cup2026` has no equivalent |
| `data` (on-chain data aggregator) | `pm-cup2026` uses its own subgraph, different protocol |
| `rtds` (real-time data stream) | Polymarket proprietary |
| `rfq` (request for quote) | Polymarket proprietary |
| `ctf` (on-chain CTF split / merge / redeem) | Better handled by the tenant wallet or front-end, not the SDK |
| `gamma` streaming | `pm-cup2026` `gamma-service` is REST-only; no stream. REST surface implemented in Phase 3a — see [`docs/gamma.md`](gamma.md). |

---

## 7. Order construction and fees

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Price precision | tick size 0.01 / 0.001 / 0.0001 | Same (`pm-cup2026` inherits the same rules) |
| Token / USDC precision | 6 decimals | Same |
| Order-field units | `size` / `amount` / `fee` human-readable, `makerAmount` / `takerAmount` in chain minimum units | Same |
| Fee-rate unit | bps (basis points); order field `feeRateBps` | Same |
| Fee algorithm | `fee = bps × amount` (flat) | **On-chain `min(p, 1−p)` adjusted formula** (see [`pm-cup2026/services/clob-service/docs/fee-algorithm.md`](../../pm-cup2026/services/clob-service/docs/fee-algorithm.md)) |
| Fee split | Single platform fee | **Split into `PLATFORM_FEE + TENANT_FEE`**, 3 on-chain transactions per side per fill |

---

## 8. SDK engineering structure

| Dimension | Polymarket V1 | pm-rs |
|-----------|---------------|-------|
| Client state machine | Type-state pattern (`Client<Unauthenticated>` → `Client<Authenticated<K>>`) | **Single `Client` struct + `Option<Credentials>` + `Option<signer_address>`** — credentials and the L1 signer address are attached at build time via `ClientBuilder::credentials` / `signer_address` (no Builder / Normal / AWS-KMS Kind tiers) |
| Builder authentication | `promote_to_builder(BuilderConfig)` + remote signer service | Not implemented (no user requirement yet) |
| AWS KMS signer | Built-in `signer-aws` example | Not implemented (callers can plug in any `alloy::signers::Signer` impl) |
| Heartbeat long connection | `heartbeats` feature flag | Phase 3 |
| WebSocket asset-ID type | `Vec<String>` | Same — shipped in Phase 3b (see [`docs/ws.md`](ws.md)) |
| WebSocket transport | `tokio_tungstenite`, type-state authenticated client | `tokio_tungstenite`, single-state client; auth carried in the first WS frame for the user channel (matches the chainup server contract, **not** HTTP `PRED_*` headers) |
| WebSocket subscribe message | Single `SubscriptionRequest` covering both channels | Two distinct envelopes — `MarketSubscribeRequest` (`type=market`, `assets_ids`) and `UserSubscribeRequest` (`type=user`, `auth.{apiKey,passphrase}`, `markets`) |
| WebSocket heartbeat | Protocol-level Ping | Text frame `"PING"` / `"PONG"` (chainup-specific; see `services/clob-service/internal/wsservice/`) |
| WebSocket runtime sub/unsub | Per-channel `subscribe` / `unsubscribe` envelopes | Same shape (`{"operation":"subscribe","assets_ids":[...]}` etc.) |
| WebSocket reconnect | Exponential backoff, re-emits subscriptions | Same |
| Public utility functions | Private | Phase 3 will expose a `utilities` module, aligned with the V2 SDK's design |

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

- `build_sign_and_post()` one-shot order placement — may land in Phase 2 (without the V2 version-mismatch retry; not relevant here).
- `user_usdc_balance()` market-buy balance auto-adjustment — reassess in Phase 2.
- The public `clob::utilities` module — Phase 3 will ship an equivalent with chainup's own fee formulas.

---

## Appendix: confirming the signing baseline

Signing parity is the single biggest behavioral difference vs Polymarket V1 and the single biggest similarity vs `pm-sdk-go`. Byte-level parity is enforced by:

```bash
cargo test -p pm-rs-clob-client --test golden_signer
```

The fixtures (`clob-client/tests/fixtures/golden.json`) are a snapshot of `pm-sdk-go/pkg/signer/testdata/golden.json`. **Any signer change must clear this test before further work is layered on top.**
