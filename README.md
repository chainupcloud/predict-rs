# pm-rs

Rust toolchain for ChainUp's [`pm-cup2026`](https://github.com/chainupcloud/pm-cup2026) prediction-market platform — a Polymarket V1-compatible CLOB with a multi-tenant `scopeId` extension.

Cargo workspace, two member crates:

| Crate | Path | Purpose |
|-------|------|---------|
| `pm-rs-clob-client` | [`clob-client/`](clob-client/) | Rust SDK for `pm-cup2026` CLOB / Gamma / WebSocket APIs. Counterpart of [`pm-sdk-go`](https://github.com/chainupcloud/pm-sdk-go); ported from Polymarket's [`rs-clob-client`](https://github.com/Polymarket/rs-clob-client) (V1) with chainup-specific extensions: `scopeId`-extended EIP-712 `ClobAuth` / `Order` domains, `PRED_*` auth headers (vs `POLY_*`), standard-base64 HMAC encoding (vs URL-safe). |
| `pm-cli` | [`cli/`](cli/) | `pm` binary — terminal client for `pm-cup2026`. Browse markets, place orders, manage positions. Counterpart of Polymarket's [`polymarket-cli`](https://github.com/Polymarket/polymarket-cli). |

## Status

**Phase 1** (done): workspace skeleton, signer verified byte-identical against `pm-sdk-go/pkg/signer/testdata/golden.json`, public CLOB REST surface (`ok`, `time`, `midpoint`, `price`, `spread`, `book`, `tick-size`, `fee-rate`, `last-trade`), matching CLI subcommands. 9/9 tests pass; 5/9 endpoints validated end-to-end against the dev server.

**Phase 2** (planned): L1 (EIP-712) + L2 (HMAC-SHA256) auth flow, API-key management, order placement and cancellation, balance/orders/trades.

**Phase 3** (planned): Gamma client (events / markets / tags), WebSocket market and user channels, interactive shell.

## Quick start

```bash
cargo build

# Public endpoints — no wallet needed
./target/debug/pm --endpoint https://clob-api.predict.prax1s.xyz ok
./target/debug/pm --endpoint https://clob-api.predict.prax1s.xyz time
./target/debug/pm --endpoint https://clob-api.predict.prax1s.xyz midpoint <TOKEN_ID>

# Output as JSON (for scripting)
./target/debug/pm -o json time
```

## Why a chainup-specific fork?

`pm-cup2026` extends Polymarket V1 with multi-tenant `scopeId` isolation:

- **`ClobAuth` EIP-712 struct** (5 fields): `address / timestamp / nonce / bytes32 scopeId / message`
- **`Order` EIP-712 struct** (13 fields): adds `bytes32 scopeId` at the end
- **Auth headers**: `PRED_*` instead of `POLY_*` (e.g. `PRED_API_KEY`, `PRED_SIGNATURE`)
- **HMAC secret**: standard base64 (Polymarket uses URL-safe)

Upstream Polymarket clients (`rs-clob-client*`) cannot talk to `pm-cup2026` without these changes. Full comparison: [`docs/diff-vs-polymarket-v1.md`](docs/diff-vs-polymarket-v1.md).

## Layout

```
pm-rs/
├── Cargo.toml              # workspace
├── CLAUDE.md
├── docs/
│   └── diff-vs-polymarket-v1.md
├── clob-client/            # SDK crate (lib)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── error.rs
│   │   ├── types.rs        # Address, Side, SignatureType, ScopeId
│   │   ├── auth.rs         # Credentials, PRED_* header constants, L2 HMAC
│   │   ├── signer.rs       # PMCup26 signer (ClobAuth + Order EIP-712)
│   │   ├── client.rs       # Client + ClientBuilder
│   │   └── clob/           # CLOB module: types + endpoint methods
│   └── tests/
│       ├── golden_signer.rs    # byte-level parity with pm-sdk-go
│       └── fixtures/
│           └── golden.json     # snapshot of pm-sdk-go's golden vectors
└── cli/                    # CLI crate (bin: pm)
    └── src/
        ├── main.rs
        ├── cli.rs
        ├── commands.rs
        └── output.rs       # table / json rendering
```

## License

MIT
