# pm-rs-clob-client

Rust SDK for [`pm-cup2026`](https://github.com/chainupcloud/pm-cup2026) prediction-market platform — a Polymarket V1-compatible CLOB extended with multi-tenant `scopeId` isolation.

Counterpart of the official Go SDK [`pm-sdk-go`](https://github.com/chainupcloud/pm-sdk-go). Ported from Polymarket's [`rs-clob-client`](https://github.com/Polymarket/rs-clob-client) with specific extensions; signer is **byte-identical** to `pm-sdk-go/pkg/signer` (golden-tested).

```toml
[dependencies]
pm-rs-clob-client = { git = "https://github.com/chainupcloud/pm-rs", package = "pm-rs-clob-client" }
```

## What you get

| Surface | Coverage | Notes |
|---------|----------|-------|
| Signer | ✅ `ClobAuth` + `Order` EIP-712, byte-aligned with `pm-sdk-go` | Three signature types: `Eoa` (0), `PolyProxy` (1), `PolyGnosisSafe` (2 — default). |
| L1 auth (API-key CRUD) | ✅ Create / derive / list / delete API keys via `/auth/*` | Uses `ClobAuth` EIP-712 challenge with `PRED_*` headers. |
| L2 auth (trading) | ✅ HMAC-SHA256 on every request | **Standard** base64 secret, not URL-safe. |
| Order placement | ✅ Limit / market / GTC / GTD / FOK / FAK + `post-only` | Server runs book walk for market orders; client anchors `makerAmount` / `takerAmount` at a price. |
| Order management | ✅ Single + batch place / cancel / cancel-all / replace | `/orders/replace` atomic swap supported. |
| Balances + positions | ✅ Collateral (USDW / USDC) + conditional (CTF) via `/balance` | Server returns `virtual_available` + `locked` breakdown. |
| Batch reads | ✅ `/midpoints`, `/prices`, `/spreads`, `/books`, `/last-trades-prices` | All five accept up to 500 ids in one call. |
| Market data | ✅ `/midpoint`, `/price`, `/spread`, `/book`, `/tick-size`, `/fee-rate`, `/last-trade-price`, `/price-history` | Intervals: `1H / 6H / 1D / 1W / 1M / ALL`. |
| Gamma (events, tags, profiles) | ✅ REST surface | Streaming variant intentionally not implemented (see Non-goals). |
| WebSocket — market | ✅ Book / price-change / last-trade-price / tick-size-change / best-bid-ask / new-market / market-resolved | Adjacent encoding `tag="event_type", content="data"` matches the live wire format. |
| WebSocket — user | ✅ Order + trade events with auto-reconnect, runtime subscribe / unsubscribe | Lean payload mode: cancellation arrives as `{id, status}` only, both shapes decode cleanly. |
| Approval helpers | ✅ Read-only `IERC20.allowance` + `IERC1155.isApprovedForAll` via alloy | The `set` flow ships through the new Safe / relayer modules — see below. |
| Safe meta-tx primitives (`safe` module) | ✅ Gnosis Safe v1.3 `SafeTransaction` + `multisend` packed encoder | `SafeTransaction::{call, delegate_call}` + `multisend::encode` produce wire-identical calldata to the front-end. |
| `SafeTx` / `LoginMessage` signing | ✅ Both EIP-712 types implemented on `PMCup26Signer` | `sign_safe_tx` and `sign_login_message` return Ethereum-`v` (27/28) signatures matching `Safe.execTransaction`'s on-chain verifier. |
| Gamma `/auth/login` (JWT) | ✅ `Client::jwt_login` | One-shot `/auth/nonce` → sign `LoginMessage` → `/auth/login` → returns Bearer token for relayer auth. |
| Relayer client (`relayer` module) | ✅ `RelayerClient::{submit, transaction, poll_until_terminal}` | Wire matches `pm-cup2026/services/relayer-service` (camelCase + the `safeTxnGas` typo). JWT or API-Key auth. |

## Quick start

### Unauthenticated client (read-only)

```rust
use pm_rs_clob_client::{Client, Endpoints};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Derive clob-api / gamma-api / clob-ws from the tenant root host.
    let client = Client::builder()
        .tenant("hermestrade.xyz")?
        .chain_id(143) // Monad
        .build()?;

    let ts = client.time().await?;
    println!("server time: {ts:?}");

    let book = client
        .book("3404193502957754813574764349521510718535214379046821174999630185571369090576")
        .await?;
    println!("best bid: {:?}", book.bids.first());
    println!("best ask: {:?}", book.asks.first());
    Ok(())
}
```

### Authenticated client (Safe wallet — default)

```rust
use pm_rs_clob_client::{
    Client, Credentials, PMCup26Signer, ScopeId, SignatureType,
};
use uuid::Uuid;

let signer = PMCup26Signer::from_hex(&std::env::var("PM_PRIVATE_KEY")?, /*chain_id=*/143)?
    .with_scope_id(ScopeId::from_hex("0x1811a132...196e95")?)
    .with_signature_type(SignatureType::PolyGnosisSafe);

// L1: mint a fresh L2 API key (uses a temporary read-only client).
let bootstrap = Client::builder().tenant("hermestrade.xyz")?.chain_id(143).build()?;
let creds: Credentials = bootstrap.create_api_key(&signer, None).await?;

// Build the trading client with both signer and credentials attached.
let client = Client::builder()
    .tenant("hermestrade.xyz")?
    .chain_id(143)
    .signer_address(signer.address())
    .credentials(creds.clone())
    .build()?;

let balance = client.balance("collateral", None).await?;
println!("USDW raw: {}", balance.balance);
```

Pre-saved credentials can be attached the same way:

```rust
let creds = Credentials::new(
    Uuid::parse_str("dcab2c0f-a6e4-44e4-a76e-2f179c53cf6a")?,
    "CWyxYqlXx8Y9IjmQaRgNieHQAD2WlHT4flgs9Vs+bqU=".into(),
    "1bc4f194e1ff8dbe1a9efdf41bef819217fafa71d1e013ec87279d5f595ccd40".into(),
);
let client = Client::builder()
    .tenant("hermestrade.xyz")?
    .credentials(creds)
    .build()?;
```

### Place a limit order (Safe wallet)

```rust
use pm_rs_clob_client::clob::order_builder::OrderBuilder;
use pm_rs_clob_client::clob::types::OrderType;
use pm_rs_clob_client::Side;
use rust_decimal_macros::dec;
use alloy::primitives::{Address, U256};

let safe: Address = "0x7e63be993c5f51547609dedfa8f2398ebf7ac2fe".parse()?;
let token: U256 = "3404193502957754813574764349521510718535214379046821174999630185571369090576"
    .parse()?;

let (_signable, signed) = OrderBuilder::limit()
    .token_id(token)
    .side(Side::Buy)
    .price(dec!(0.10))
    .size(dec!(5))               // minimum is 5 shares
    .fee_rate_bps(20)
    .maker(safe)                 // Safe holds the funds; EOA signer is implicit
    .build_and_sign(&signer)?;

let resp = client
    .post_order(signed, OrderType::Gtc, /*post_only=*/false, /*owner=*/"")
    .await?;
println!("orderID: {}", resp.order_id);
```

### Place a market order

```rust
let (_signable, signed) = OrderBuilder::market()
    .token_id(token)
    .side(Side::Buy)
    .price(dec!(0.75))           // limit price — anchor for makerAmount/takerAmount
    .usdc(dec!(3.75))            // amount in USDW (BUY only)
    .fee_rate_bps(20)
    .maker(safe)
    .build_and_sign(&signer)?;

client.post_order(signed, OrderType::Fak, false, "").await?;
```

The server runs the actual book walk, but the signed envelope still carries `makerAmount` / `takerAmount` anchored at the price. The client validates lot size before signing: `amount / price` must be a multiple of 0.01.

### Safe-mode write via the relayer (path B)

For any on-chain write — token approvals, CTF split / merge / redeem — only `signatureType=2` is accepted. Instead of broadcasting from the EOA, sign a `Safe.execTransaction` payload and submit it to the `relayer-service`; the relayer broadcasts from its own gas-key pool, so the user spends zero collateral.

```rust
use pm_rs_clob_client::{Client, PMCup26Signer};
use pm_rs_clob_client::safe::SafeTransaction;
use pm_rs_clob_client::relayer::{SafeTxParams, SubmitRequest, SubmitType};

let client = Client::builder()
    .tenant("hermestrade.xyz")?
    .chain_id(143)
    .build()?;

let signer = PMCup26Signer::from_hex(&eoa_private_key, 143)?
    .with_scope_id(scope_id);

// 1. Build the inner call. Here: USDW.approve(NegRiskCtfExchange, MAX).
let approve_data = /* encode usdw.approve(spender, MAX) */;
let safe_tx = SafeTransaction::call(usdw, approve_data, safe_nonce);

// 2. Sign the SafeTx EIP-712 payload (returns 65 bytes with v in {0x1b, 0x1c}).
let signature = signer.sign_safe_tx(safe_address, &safe_tx)?;

// 3. Get a JWT for the relayer (gamma-service /auth/nonce + /auth/login).
let jwt = client.jwt_login(&signer, "hermestrade.xyz", "https://hermestrade.xyz").await?;

// 4. Submit + poll until terminal.
let relayer = client.relayer()?.with_token(&jwt);
let req = SubmitRequest {
    from: format!("{:#x}", signer.address()),
    to: format!("{:#x}", safe_tx.to),
    proxy_wallet: format!("{:#x}", safe_address),
    data: format!("0x{}", hex::encode(&safe_tx.data)),
    nonce: Some(safe_tx.nonce.to_string()),
    signature: format!("0x{}", hex::encode(signature)),
    signature_params: serde_json::to_value(SafeTxParams::relayer_pays(false))?,
    r#type: SubmitType::Safe,
    scope_id: Some(format!("{:#x}", signer.scope_id().as_b256())),
    metadata: Some("approve".to_owned()),
};
let resp = relayer.submit(&req).await?;
let final_tx = relayer
    .poll_until_terminal(&resp.transaction_id,
                         std::time::Duration::from_secs(3),
                         std::time::Duration::from_secs(120))
    .await?;
println!("hash={} state={:?}", final_tx.transaction_hash, final_tx.state);
```

For batching multiple ops (e.g. fresh-wallet onboarding — USDW.approve to N spenders + CTF.setApprovalForAll to N operators), use [`safe::multisend::encode`] to pack `SafeSubOp::call(target, data)` into a single `DelegateCall` to the MultiSend contract:

```rust
use pm_rs_clob_client::safe::multisend::{self, SafeSubOp};

let ops = vec![
    SafeSubOp::call(usdw, encode_approve(ctf_exchange, U256::MAX)),
    SafeSubOp::call(usdw, encode_approve(neg_risk_exchange, U256::MAX)),
    SafeSubOp::call(ctf, encode_set_approval_for_all(ctf_exchange, true)),
    SafeSubOp::call(ctf, encode_set_approval_for_all(neg_risk_exchange, true)),
];
let packed = multisend::encode(&ops)?;
let safe_tx = SafeTransaction::delegate_call(multisend_address, packed, safe_nonce);
```

The CLI's `pm approve set --asset all` and `pm ctf {redeem,split,merge}` commands are full reference implementations of this pattern — see `cli/src/safe_exec.rs` for the shared plumbing.

### WebSocket — market channel

```rust
use futures::StreamExt;
use pm_rs_clob_client::{MarketSubscribeOpts, clob::ws::types::request::MarketLevel};

let ws = client.clob_ws()?; // requires `Endpoints::ws_endpoint`
let mut stream = ws
    .subscribe_market(
        vec!["3404...0576".into()],
        MarketSubscribeOpts::default()
            .with_initial_dump(true)
            .with_level(MarketLevel::Two),
    )
    .await?;

while let Some(event) = stream.next().await {
    match event? {
        MarketEvent::Book(b) => println!("book {} bids={} asks={}", b.asset_id, b.bids.len(), b.asks.len()),
        MarketEvent::PriceChange(pc) => println!("price_change {}", pc.market),
        _ => {}
    }
}
```

### WebSocket — user channel (requires credentials)

```rust
// Credentials attached at builder time are forwarded to clob_ws() automatically.
let ws = client.clob_ws()?;
let mut stream = ws.subscribe_user(vec!["0xcid".into()]).await?;

while let Some(event) = stream.next().await {
    match event? {
        UserEvent::Order(o) => println!("order {} -> {:?}", o.id, o.status),
        UserEvent::Trade(t) => println!("trade {} {:?} {}", t.id, t.status, t.match_type),
    }
}
```

## Wire-level differences vs Polymarket V1

| Topic | Polymarket V1 | pm-cup2026 (this SDK) |
|-------|---------------|------------------------|
| `ClobAuth` struct | 4 fields | **5 fields** — `bytes32 scopeId` inserted between `nonce` and `message`. |
| `Order` struct | 12 fields | **13 fields** — `bytes32 scopeId` appended at the end. |
| `Order` EIP-712 domain `name` | `"Polymarket CTF Exchange"` | `"Prediction Market Protocol"` |
| Auth headers | `POLY_API_KEY` / `POLY_SIGNATURE` / … | **`PRED_API_KEY` / `PRED_SIGNATURE` / …** |
| HMAC base64 | URL-safe | **Standard** |
| Contract addresses | Hard-coded `phf_map!` in `lib.rs` | Runtime config — caller supplies them. Example YAMLs under [`../examples/networks/`](../examples/networks/). |

Full diff table: [`../docs/diff-vs-polymarket-v1.md`](../docs/diff-vs-polymarket-v1.md).

## Module map

```
clob-client/src/
├── lib.rs              — re-exports + top-level docs
├── error.rs            — Error enum (thiserror)
├── types.rs            — Address, Side, SignatureType, ScopeId, AssetType
├── endpoints.rs        — Endpoints (clob / gamma / ws) + tenant derivation
├── auth.rs             — PRED_* header constants, L2 HMAC sign
├── signer.rs           — PMCup26Signer (ClobAuth / Order / SafeTx / LoginMessage EIP-712)
├── client.rs           — Client + ClientBuilder (REST surface)
├── clob/
│   ├── types.rs        — Order, OrderBookSummary, Trade, …
│   ├── order_builder.rs — OrderBuilder (limit / market)
│   └── ws/
│       ├── client.rs       — ClobWebSocketClient
│       ├── subscription.rs — MarketStream / UserStream
│       └── types/{request, response}.rs
├── gamma/
│   ├── client.rs       — Gamma sub-client (events / markets / tags / profiles)
│   └── types/{request, response}.rs
├── data/               — data-service client (portfolio / activity / leaderboards)
├── ws/                 — shared WS transport (auto-reconnect)
├── safe/               — Gnosis Safe v1.3 SafeTransaction + multisend encoder
└── relayer/            — RelayerClient (submit / poll Safe meta-tx)
```

## Key types

Re-exported from the crate root:

```rust
pub use crate::{
    Address, Client, ClientBuilder, ClobWebSocketClient, Credentials, Endpoints, Error,
    MarketSubscribeOpts, PMCup26Signer, Result, ScopeId, Side, SignatureType,
};
pub use crate::clob::types::{Order, OrderBuilder, OrderBookSummary, OrderType, ...};
pub use crate::clob::ws::types::response::{
    MarketEvent, OrderEvent, OrderSide, OrderStatus, TradeEvent, TradeStatus, UserEvent,
};
```

## Live-tested

Driven against `clob-api.hermestrade.xyz` on Monad (chainId 143) with a Gnosis-Safe maker on 2026-05-19 and 2026-05-20:

- Single + batch limit / market / GTD orders end-to-end (place → match → cancel)
- Two real trades minted (`match_type: MINT` — negRisk complement-mint settlement)
- WS market + user channels including the post-fill `MATCHED` → `CONFIRMED` lifecycle
- Approval cross-checks via on-chain `IERC20.allowance` + `IERC1155.isApprovedForAll`
- Safe wallet pre-deployment + pre-approval verified via on-chain `Safe.getOwners()`

Seven SDK fixes landed during the runs in response to live-wire surprises (see commit log `1a97466..HEAD`). The most common surprise: the wire schema is leaner than the asyncapi spec — most fields default-able, sometimes only `{id, status}` is sent.

## Non-goals

Things this SDK intentionally does **not** ship — typically because the backend doesn't expose them, not because we ran out of time:

- **Hard-coded contract addresses** — Polymarket's `phf_map!` in `lib.rs` is rejected by design. Everything comes from runtime config.
- **`/markets`, `/sampling-markets`, `/simplified-markets`** — market discovery goes through Gamma.
- **`/rewards`, `/earnings/total`, `/reward-percentages`** + 4 more — tenants run their own incentive logic.
- **`/notifications`, `/closed-only-mode`, `/account-status`, `/geoblock`** — not exposed by the CLOB.
- **Gamma streaming** — Gamma is REST-only.
- **EOA-broadcast `ctf split / merge / redeem`** — only `signatureType=2` (Safe) is supported. The SDK provides the building blocks (`safe`, `relayer`, `sign_safe_tx`, `jwt_login`) so callers can compose any Safe meta-tx — but it does not ship an EOA-direct broadcaster, since the backend does not honour those orders.
- **Polymarket bridge, rtds, rfq** — Polymarket-proprietary endpoints not present here.

If the backend later ships any of these, the SDK can be extended without breaking the existing surface.

## Testing

```bash
# Full workspace + offline tests
cargo test --workspace

# Live network smokes (require credentials)
cargo test --workspace -- --ignored

# Golden-signer regression (must always pass)
cargo test -p pm-rs-clob-client --test golden_signer

# Live WS smoke (requires a running clob-ws endpoint)
cargo test --workspace --test ws_market_smoke -- --ignored
```

Golden vectors live at [`tests/fixtures/golden.json`](tests/fixtures/golden.json) — copied from `pm-sdk-go/pkg/signer/testdata/golden.json`. Any change to the signer requires a coordinated update to that file.

## Feature flags

| Flag | Default | Effect |
|------|---------|--------|
| `tracing` | off | Emits `tracing` events on every outbound HTTP / WS frame. Costs no runtime when off (`tracing` is `optional = true`). |

## Minimum Supported Rust Version

**1.88**. Pinned via `rust-version` in the workspace `Cargo.toml`. Older toolchains may compile but are not tested.

## License

MIT — see [`LICENSE`](../LICENSE) at the repo root.
