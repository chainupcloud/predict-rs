# predict-rs

Rust toolchain for [`pm-cup2026`](https://github.com/chainupcloud/pm-cup2026) prediction-market platform — a Polymarket V1-compatible CLOB with a multi-tenant `scopeId` extension.

Cargo workspace, two member crates:

| Crate | Path | Purpose |
|-------|------|---------|
| `predict-rs-clob-client` | [`clob-client/`](clob-client/) | Rust SDK for `pm-cup2026` CLOB / Gamma / WebSocket APIs. Counterpart of [`pm-sdk-go`](https://github.com/chainupcloud/pm-sdk-go); ported from Polymarket's [`rs-clob-client`](https://github.com/Polymarket/rs-clob-client) (V1) with specific extensions: `scopeId`-extended EIP-712 `ClobAuth` / `Order` domains, `PRED_*` auth headers (vs `POLY_*`), standard-base64 HMAC encoding (vs URL-safe). |
| `predict-cli` | [`cli/`](cli/) | `predict-cli` binary — terminal client for `pm-cup2026`. Browse markets, place orders, manage positions. Counterpart of Polymarket's [`polymarket-cli`](https://github.com/Polymarket/polymarket-cli). |

## Status

Pre-1.0. The SDK and CLI are functionally complete against the dev backend on Monad (chainId 143) and OP Sepolia (chainId 11155420); the wire surface is live-tested but unstable. Expect minor breaking changes until a 1.0 tag lands.

Shipped surface:

- **Signer** — `ClobAuth` / `Order` / `SafeTx` / `LoginMessage` EIP-712 types, byte-identical against `pm-sdk-go/pkg/signer/testdata/golden.json` (golden test gates every change).
- **Auth** — L1 EIP-712 challenge, L2 HMAC-SHA256, API-key CRUD.
- **Orders** — limit / market / GTC / GTD / FOK / FAK / post-only, single + batch place / cancel / cancel-all / replace.
- **Reads** — midpoint / price / spread / book / tick-size / fee-rate / last-trade + batch variants (≤ 500 ids).
- **Gamma** — events / markets / tags / profiles (REST).
- **WebSocket** — market + user channels with auto-reconnect.
- **CTF** — `condition-id` / `position-id` / `collection-id` helpers; `split` / `merge` / `redeem` via Safe meta-tx through the relayer.
- **Approvals** — `IERC20.allowance` + `IERC1155.isApprovedForAll` reads; `set` via Safe MultiSend through the relayer.

## Install

One-line install (macOS / Linux, x86_64 / arm64) — downloads the latest release binary, verifies its sha256, installs to `/usr/local/bin`:

```bash
curl -sSfL https://raw.githubusercontent.com/chainupcloud/predict-rs/main/install.sh | sh
```

Requires at least one published [release](https://github.com/chainupcloud/predict-rs/releases) (`v*` tag → [`release.yml`](.github/workflows/release.yml) builds the four target tarballs + `checksums.txt`). To build from source instead, see Quick start below.

## Quick start

```bash
cargo build

# Point the CLI at a tenant — derives clob-api / gamma-api / clob-ws subdomains automatically.
./target/debug/predict-cli --tenant hermestrade.xyz ok
./target/debug/predict-cli --tenant hermestrade.xyz time
./target/debug/predict-cli --tenant hermestrade.xyz endpoints
./target/debug/predict-cli --tenant hermestrade.xyz midpoint <TOKEN_ID>

# Or pass the CLOB URL directly (useful for non-canonical hostnames / dev setups).
./target/debug/predict-cli --clob-endpoint https://clob-api.hermestrade.xyz time

# Output as JSON (for scripting).
./target/debug/predict-cli --tenant hermestrade.xyz -o json time
```

Env vars `PM_TENANT`, `PM_CLOB_ENDPOINT`, `PM_GAMMA_ENDPOINT`, `PM_WS_ENDPOINT`, `PM_CHAIN_ID`, `PM_SCOPE_ID` mirror the flags.

### SDK usage

The SDK mirrors `pm-sdk-go`'s `WithEndpoints(clob, gamma, ws)` shape:

```rust
use predict_rs_clob_client::{Client, Endpoints};

// Explicit three-URL form
let client = Client::builder()
    .endpoints(Endpoints::new(
        "https://clob-api.hermestrade.xyz",
        "https://gamma-api.hermestrade.xyz",
        "wss://clob-ws.hermestrade.xyz",
    )?)
    .chain_id(143)             // Monad chain id — confirm at use time
    .user_agent("my-app/1.0")
    .build()?;

// Or derive from tenant host (canonical pattern)
let client = Client::builder()
    .tenant("hermestrade.xyz")?
    .chain_id(143)
    .build()?;
```

Reference network configs (NOT hard-coded in the SDK — caller supplies them at runtime) live under [`examples/networks/`](examples/networks/).

## Why a platform-specific fork?

`pm-cup2026` extends Polymarket V1 with multi-tenant `scopeId` isolation:

- **`ClobAuth` EIP-712 struct** (5 fields): `address / timestamp / nonce / bytes32 scopeId / message`
- **`Order` EIP-712 struct** (13 fields): adds `bytes32 scopeId` at the end
- **Auth headers**: `PRED_*` instead of `POLY_*` (e.g. `PRED_API_KEY`, `PRED_SIGNATURE`)
- **HMAC secret**: standard base64 (Polymarket uses URL-safe)

Upstream Polymarket clients (`rs-clob-client*`) cannot talk to `pm-cup2026` without these changes. Full comparison: [`docs/diff-vs-polymarket-v1.md`](docs/diff-vs-polymarket-v1.md).

## Layout

```
predict-rs/
├── Cargo.toml              # workspace
├── LICENSE
├── README.md               # this file
├── CLAUDE.md
├── docs/                   # auth-flow / orders / ws / gamma / wallet / diff-vs-polymarket-v1
├── examples/networks/      # reference network YAMLs (Monad / OP Sepolia / …)
├── clob-client/            # SDK crate (predict-rs-clob-client) — see clob-client/README.md
└── cli/                    # CLI crate (binary: predict-cli) — see cli/README.md
```

## License

MIT — see [`LICENSE`](LICENSE).
