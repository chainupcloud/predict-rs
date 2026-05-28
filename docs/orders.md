# Orders

`pm-rs-clob-client` exposes order construction, signing, submission, cancellation, and
order / trade query endpoints, plus the matching `pm order …` / `pm trade …` CLI
subcommands.

## Lifecycle

```
┌────────────┐    builder    ┌──────────────┐  sign  ┌──────────────┐  L2 POST
│ user input ├──────────────▶│ SignableOrder├───────▶│  SignedOrder ├──────────▶ /order
└────────────┘               └──────────────┘        └──────────────┘
                                                                            ┌──────────┐
                                                              server ─────▶│ Match    │
                                                                            │ engine   │
                                                                            └────┬─────┘
                                                                                 │
                                                                                 ▼
                                            GET /orders   ◀──── DB / memory ────┤
                                            GET /order/{}        order rows     │
                                            DELETE /order        cancel queue ──┘
                                            DELETE /orders
                                            DELETE /cancel-all
                                            DELETE /cancel-market-orders
                                            GET /trades
                                            GET /order-scoring
                                            POST /heartbeats
```

## Amount math

`price` ∈ `(0, 1)`, `size` ∈ `(0, ∞)` (human-readable shares; max 2 decimals). The SDK
converts to 6-decimal raw integers (matching the USDC + CTF token precision):

| Side | `makerAmount`                | `takerAmount`              |
|------|------------------------------|----------------------------|
| BUY  | `price × size × 10^6`        | `size × 10^6`              |
| SELL | `size × 10^6`                | `price × size × 10^6`      |

The conversion uses `Decimal::trunc_with_scale(6).mul(10^6).trunc()` — floor truncation,
not rounding, matching `pm-sdk-go::toBaseUnits`.

## Tick-size table

| Tick size | Price decimals | Size decimals | Amount decimals |
|-----------|----------------|---------------|-----------------|
| 0.01      | 2              | 2             | 4               |
| 0.001     | 3              | 2             | 5               |
| 0.0001    | 4              | 2             | 6               |

When `OrderBuilder::minimum_tick_size(...)` is set (typically by callers fetching
`GET /tick-size` for the token), the SDK enforces the price decimals + bounds. Out of
band, the server enforces the same rule and rejects orders that don't comply.

## Fee algorithm

`feeRateBps` is required on every order. The server applies the
`min(p, 1-p)`-adjusted formula to compute the actual fee at fill time:

- **BUY** fee in tokens: `min(p, 1-p) / p × size × bps / 10000`
- **SELL** fee in USDC: `min(p, 1-p) × size × bps / 10000`

This is the same formula on-chain as `CalculatorHelper.calculateExchangeFee`. See
`pm-cup2026/services/clob-service/docs/fee-algorithm.md` for the full derivation.

## Signature normalisation

The 65-byte `r||s||v` signature output by the Rust signer carries `v ∈ {0, 1}` (matching
go-ethereum's `crypto.Sign`). The SDK applies the `+ 27` normalisation pm-sdk-go's
`normalizeECDSAv` performs, so the on-wire `v` is in `{0x1b, 0x1c}` — required by the
on-chain `ECDSA.recover` path that the relayer takes for both EOA and
`POLY_GNOSIS_SAFE` signature types. The server-side L2 verifier accepts both `{0, 1}`
and `{27, 28}`; we standardise on `{27, 28}` for end-to-end parity.

## Safe-wallet architecture (default)

With `signatureType = 2` (PolyGnosisSafe):

- `maker` = **Safe address** (CREATE2-derived from `keccak256(abi.encode(signer, scopeId))`).
- `signer` = EOA address (the private key holder, a 1-of-1 Safe owner).

The SDK requires `.maker(<Safe address>)` to be set explicitly when
`signature_type = PolyGnosisSafe`; client-side Safe-address derivation is a follow-up.

For `signatureType = 0` (EOA) the SDK enforces `maker == signer` client-side.

## SDK example

```rust
use pm_rs_clob_client::{
    Client, ClientBuilder, Endpoints, PMCup26Signer, Side, SignatureType,
};
use pm_rs_clob_client::clob::types::OrderType;
use pm_rs_clob_client::types::{ScopeId, U256};
use rust_decimal_macros::dec;

# async fn run() -> pm_rs_clob_client::Result<()> {
let signer = PMCup26Signer::from_hex(&std::env::var("PM_PRIVATE_KEY").unwrap(), 11_155_420)?
    .with_scope_id(ScopeId::from_hex("0x42").unwrap())
    .with_exchange("0xC6e9081EcaD84AfB3a772933Fb865AB8A9C317d9".parse().unwrap());

let client = Client::builder()
    .endpoints(Endpoints::from_tenant("hermestrade.xyz")?)
    .chain_id(11_155_420)
    .signer_address(signer.address())
    .build()?;

// Discover the per-token fee + tick size up front.
let fee = client.fee_rate("100").await?;
let tick = client.tick_size("100").await?;

// Build, sign, and submit.
let (signable, signed) = client
    .limit_order()
    .token_id(U256::from(100u64))
    .price(dec!(0.34))
    .size(dec!(100))
    .side(Side::Buy)
    .order_type(OrderType::Gtc)
    .fee_rate_bps(fee.fee_rate_bps)
    .minimum_tick_size(tick.minimum_tick_size)
    .maker(signer.address())              // EOA → maker == signer
    .signature_type(SignatureType::Eoa)
    .build_and_sign(&signer)?;

let resp = client
    .post_order(signed, signable.order_type, signable.post_only, signable.owner)
    .await?;
println!("orderID={} status={}", resp.order_id, resp.status);
# Ok(())
# }
```

## CLI examples

```bash
# Place a limit BUY for 100 @ 0.34, scope 0x42 on OP Sepolia.
pm --tenant hermestrade.xyz \
   --chain-id 11155420 --scope-id 0x42 \
   --private-key "$PM_PRIVATE_KEY" \
   --exchange-address 0xC6e9... \
   order create \
   --token 100 --side buy --price 0.34 --size 100 \
   --fee-rate-bps 50 \
   --maker 0xSafe... \
   --signature-type gnosis-safe

# Dry-run: print the signed envelope JSON, do NOT POST.
pm ... order create --token 100 ... --dry-run

# Cancel single / batch / market.
pm ... order cancel snowflake-1
pm ... order cancel-many id1,id2,id3
pm ... order cancel-market --asset-id 100
pm ... order cancel-all

# Open orders + filters.
pm ... order list --market 0xcondition --status all
pm ... order get snowflake-1

# Replace (cancel old + place new) — `--orders-file` accepts an array of SendOrder
# envelopes (produced by `pm order create --dry-run`).
pm ... order replace --cancel id1,id2 --orders-file new.json

# Trades.
pm ... trade --asset-id 100 --from-id 1000 --limit 50
pm ... trade --asset-id 100 --builder              # GET /builder/trades

# Maker-program ops.
pm ... order scoring snowflake-1
pm ... heartbeat
```

## Wire-format reminder

- `order.tokenID` is **camelCase** (`tokenID`), not `token_id`. Same for `makerAmount` /
  `takerAmount` / `feeRateBps` / `signatureType` / `scopeId`.
- `signatureType` is the **string form** of the enum: `"0"` / `"1"` / `"2"`.
- `side` is `"BUY"` or `"SELL"` (uppercase).
- All numeric fields (`salt` / `tokenID` / `makerAmount` / `takerAmount` / `expiration` /
  `nonce` / `feeRateBps`) are JSON strings.
- `scopeId` is `0x` + 64 hex chars; omitted when the scope is zero.
- `signature` is `0x` + 130 hex chars (`r||s||v` with `v ∈ {27, 28}`).
- `next_cursor: "LTE="` means end-of-stream.

## Differences vs Polymarket V1

- Salt = `time.Now().UnixNano()` masked to 53 bits (matches pm-sdk-go); rs-clob-client
  uses a randomised mask of `seconds × rand_f64`.
- `OrderBuilder` does NOT walk the order book client-side for market orders. The
  server does the actual book traversal; the SDK only signs the limit-price anchor.
- The salt / fee-rate fields are still uint256 strings on the wire but the server accepts
  any decimal up to 78 digits (`clob_orders.salt VARCHAR(78)`); the SDK pins salt to
  `u64::masked_53_bits` per pm-sdk-go to keep numeric round-trips clean.
- Builder-program client-side flows are out of scope; `GET /builder/trades` is exposed
  (since it's just a query), `POST` paths using `PRED_BUILDER_*` headers are not
  implemented.
