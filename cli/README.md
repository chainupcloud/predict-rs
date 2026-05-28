# pm-cli (`pm`)

Terminal client for [`pm-cup2026`](https://github.com/chainupcloud/pm-cup2026) prediction-market platform. Browse markets, place orders, manage positions — counterpart of Polymarket's [`polymarket-cli`](https://github.com/Polymarket/polymarket-cli), with feature parity for everything the backend exposes (see [Non-goals](#non-goals) for what it deliberately skips).

```bash
$ pm --tenant hermestrade.xyz time
2026-05-20T02:25:10Z

$ pm --tenant hermestrade.xyz book 3404...0576
asks:
  0.75 × 5
bids:
  0.68 × 10
```

## Install

### Build from source

```bash
git clone https://github.com/chainupcloud/pm-rs.git
cd pm-rs
cargo build --release
install -m 0755 target/release/pm ~/.local/bin/pm
```

Requires Rust 1.88+ (pinned via `rust-version` in the workspace `Cargo.toml`).

## Quick start

### Read-only — no wallet needed

```bash
# Point at a tenant — clob-api / gamma-api / clob-ws are derived automatically.
pm --tenant hermestrade.xyz ok                  # server health
pm --tenant hermestrade.xyz time
pm --tenant hermestrade.xyz endpoints           # show derived URLs + chain id
pm --tenant hermestrade.xyz book   <TOKEN_ID>
pm --tenant hermestrade.xyz midpoint <TOKEN_ID>
pm --tenant hermestrade.xyz gamma events get how-many-fed-rate-cuts-in-2026-pm-406282
```

Or supply the CLOB URL directly (useful for non-canonical hostnames or local dev):

```bash
pm --clob-endpoint https://clob-api.hermestrade.xyz time
```

JSON for scripts:

```bash
pm --tenant hermestrade.xyz -o json book 3404...0576 | jq '.bids[0]'
```

### Trading — wallet + L2 credentials

```bash
# 1. Wallet — pick one
pm wallet create                                # generates a fresh EOA, stores 0600
pm wallet import 0xYOURKEY                      # or import an existing one
pm wallet set-safe 0xYOUR_SAFE_ADDRESS          # required when signature-type = gnosis-safe
pm wallet show                                  # eoa + safe + source

# 2. Create an L2 API key (writes credentials.json mode 0600)
pm auth create-key --output json > credentials.json

# 3. Trade
export PM_TENANT=hermestrade.xyz
export PM_CHAIN_ID=143
export PM_SCOPE_ID=0x1811a132dd725e2c40475aa52df39025b36544f7a70825968e32b28da2196e95
export PM_CREDENTIALS_FILE=$PWD/credentials.json

pm balance --asset-type collateral
pm order create --token 3404...0576 --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker 0xYOUR_SAFE
pm order list
pm order cancel <ORDER_ID>
```

The first run prompts auto-pick a sensible config dir (`~/.config/pm` on Linux, mode 0700). Override with `--config-dir` or `PM_CONFIG_DIR`.

## Configuration

### Resolution order

Every connection flag (`--tenant`, `--clob-endpoint`, `--chain-id`, `--scope-id`, `--private-key`, …) resolves in this order:

1. CLI flag — wins.
2. Env var — `PM_TENANT`, `PM_CLOB_ENDPOINT`, `PM_CHAIN_ID`, `PM_SCOPE_ID`, `PM_PRIVATE_KEY`, `PM_SIGNATURE_TYPE`, `PM_EXCHANGE_ADDRESS`, `PM_CONFIG_DIR`, `PM_CREDENTIALS_FILE`, `PM_OUTPUT`.
3. Stored config — `<config-dir>/config.toml` (written by `pm wallet …`).

Empty values are treated as unset.

### Signature types

| Value | Type | Use when |
|-------|------|----------|
| `eoa` | 0 — direct EOA signing | Funds held in the same EOA that signs. Polymarket-style trading wallet. |
| `proxy` | 1 — Polymarket proxy wallet | Legacy / interop. |
| `gnosis-safe` (**default**) | 2 — 1-of-1 Gnosis Safe | **Default.** EOA signs; the Safe is the `maker` and holds the funds. |

The default is `gnosis-safe`. Persist a different choice with `pm wallet create --signature-type eoa`, or override per-invocation via `--signature-type <eoa|proxy|gnosis-safe>` / `PM_SIGNATURE_TYPE`.

### `scopeId` — multi-tenant isolation

`scopeId` is a `bytes32` value embedded in every signed `ClobAuth` and `Order`. Two clients on the same EOA but different scopes derive different L2 keys and never share order state. Fetch the right one with:

```bash
# From the server (returns the canonical scope for your tenant)
curl https://clob-api.<tenant>/auth/nonce | jq -r .scopeId

# Or via the CLI
pm auth nonce | grep scopeId
```

Set it via flag, env var, or `pm wallet create --scope-id 0x…`.

### Network config (`approve check`, `approve set`, `ctf …`)

Every command that touches the chain (`pm approve check / set`, `pm ctf redeem / split / merge / collection-id`) needs a tenant network YAML. One ships at [`examples/networks/monad-hermestrade.yaml`](../examples/networks/monad-hermestrade.yaml):

```bash
pm approve check --network-config examples/networks/monad-hermestrade.yaml
pm approve set   --network-config examples/networks/monad-hermestrade.yaml --execute
pm ctf split     --network-config examples/networks/monad-hermestrade.yaml --condition-id 0x… --partition 1,2 --amount 1000 --execute
```

The YAML is the single source of truth for chain id, RPC URL, contract addresses (USDW, CTF, exchanges), and the relayer endpoint. It's the same shape the backend deploy tooling uses.

## Commands

### Market data — public, no auth

| Command | What it does |
|---------|--------------|
| `pm ok` / `pm time` | Server health + clock |
| `pm endpoints` | Show the derived clob / gamma / ws URLs + chain id |
| `pm midpoint <TOKEN>` | Single-token midpoint |
| `pm price <TOKEN> --side buy` | Last price (one side) |
| `pm spread <TOKEN>` | Best-bid / best-ask + spread |
| `pm book <TOKEN>` | Top-of-book depth |
| `pm tick-size <TOKEN>` | Active tick size |
| `pm fee-rate <TOKEN>` | Fee rate bps |
| `pm last-trade <TOKEN>` | Last trade price |
| `pm price-history <TOKEN> --interval 1h \| 6h \| 1d \| 1w \| 1m \| all` | Historical price points |
| `pm midpoints t1 t2 ...` | Batch (≤ 500 tokens) |
| `pm prices t1:buy t2:sell ...` | Batch — per-token side selectable |
| `pm spreads t1 t2 ...` | Batch spreads |
| `pm books t1:buy t2:sell ...` | Batch books |
| `pm last-trades t1 t2 ...` | Batch last trades |

### Gamma — event / market discovery

```bash
pm gamma events list --limit 10
pm gamma events get how-many-fed-rate-cuts-in-2026-pm-406282
pm gamma events tags 291
pm gamma markets get <CONDITION_ID>            # or slug
pm gamma profiles get <SAFE_ADDRESS>
pm gamma tags list
```

Gamma is REST-only; there is no streaming variant.

### Wallet

```bash
pm wallet create [--force]                    # random EOA, mode 0600
pm wallet import 0xHEXKEY
pm wallet address                             # print EOA only
pm wallet show                                # eoa + safe + source
pm wallet reset                               # delete config
pm wallet set-safe 0xSAFE                     # store Safe address (gnosis-safe mode)
pm wallet detect-safe                         # ask the server for the Safe linked to the API key
```

### Authentication (L1 + L2 API keys)

```bash
pm auth nonce                                 # nonce + scopeId for the current EOA
pm auth derive-key                            # deterministic L2 key derivation (no server write)
pm auth create-key                            # POST /auth/api-key
pm auth list-keys
pm auth delete-key <UUID> [--nonce N]
```

### Trading

```bash
# Place a limit order (default GTC)
pm order create --token <T> --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker <SAFE>

# postOnly / GTD
pm order create --token <T> --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker <SAFE> --post-only
pm order create --token <T> --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker <SAFE> \
                --order-type gtd --expiration $(( $(date +%s) + 600 ))

# Market order (BUY only — amount denominated in USDW; server runs the book walk)
pm order create --token <T> --side buy --amount 3.75 --price 0.75 \
                --fee-rate-bps 20 --maker <SAFE> --market

# Batch place
pm order post-batch --tokens t1,t2 --prices 0.10,0.05 --sizes 5,5 \
                    --side buy --fee-rate-bps 20 --maker <SAFE>

# Manage
pm order list
pm order get <ID>
pm order cancel <ID>
pm order cancel-many <ID1>,<ID2>,...
pm order cancel-all
pm order replace --orders-file replace.json   # atomic cancel + re-place

# Dry-run anywhere — prints the signed envelope, does NOT post
pm order create ... --dry-run -o json
```

#### Lot size + minimum size

- **Minimum order size: 5 shares.** Smaller orders return `ORDER_SIZE_TOO_SMALL`.
- **Lot size: 0.01.** For market orders, `amount / price` must round to a multiple of 0.01.

#### Fee model (live finding)

Fees are deducted **in shares on the receiving side**, not in USDW. A BUY 5 × 0.09 with `--fee-rate-bps 20`:
- USDW spent: 5 × 0.09 = 0.45 (exact)
- Tokens received: 5 - 0.01 = 4.99 (fee in shares)

### Trades + balance

```bash
pm trade                                      # your trade history
pm trade --token <T>
pm balance --asset-type collateral
pm balance --asset-type conditional --token <T>
pm balance --update                           # force refresh from the server
pm fee-rate                                   # account-level fee tier
pm heartbeat                                  # server-side liveness ping
```

### Approval helpers

```bash
# Read-only — query allowance / setApprovalForAll status for every YAML target.
pm approve check --network-config examples/networks/monad-hermestrade.yaml

# Write — issue approvals via Safe meta-tx through the relayer.
# Defaults to dry-run (signs locally + prints the SubmitRequest, never POSTs).
pm approve set --network-config examples/networks/monad-hermestrade.yaml

# Default `--asset all` batches USDW.approve(target, MAX) +
# CTF.setApprovalForAll(target, true) for every approval target into one
# MultiSend. This is what a fresh community wallet needs.
pm approve set --network-config examples/networks/monad-hermestrade.yaml --execute

# Narrow the batch:
pm approve set --asset usdw --execute              # USDW.approve only
pm approve set --asset ctf  --execute              # CTF.setApprovalForAll only
pm approve set --spender 0x017641…  --execute      # one target only (single Call, not MultiSend)
pm approve set --spender 0xd77d5500…  --execute    # add ConditionalTokens — prerequisite for pm ctf split/merge
```

Gas is paid by the relayer's key pool; the user spends **zero collateral**. Polling is built-in: `--poll-interval-secs` (default 2) and `--poll-timeout-secs` (default 60) control how long the CLI waits for `STATE_CONFIRMED`.

### Safe-mode writes via the relayer (path B)

Every `pm` write command runs through the same flow — the only difference between `approve set` and `ctf {redeem,split,merge}` is the encoded calldata:

1. **JWT login** — `Client::jwt_login` hits gamma-service `/auth/nonce` → signs an EIP-712 `LoginMessage` → `POST /auth/login` → returns a Bearer JWT.
2. **Safe nonce** — read `Safe.nonce()` from the YAML's `network.rpc_url`.
3. **Build SafeTx** — either a single `Call` (one op) or `DelegateCall` to MultiSend (N ops).
4. **Sign** — `PMCup26Signer::sign_safe_tx` produces 65 bytes with Ethereum `v` in {0x1b, 0x1c}.
5. **Submit** — `POST relayer /submit` with the signed `SubmitRequest`. Returns a `transactionID` immediately; the relayer broadcasts asynchronously.
6. **Poll** — `GET relayer /transaction?id=…` until terminal: `STATE_CONFIRMED`, `STATE_FAILED`, or `STATE_DROPPED`. CLI surfaces the final tx hash + state.

You don't pay gas (the relayer covers it from its own key pool). You don't need any external broadcaster. All you need is the EOA private key + the Safe address.

### WebSocket

```bash
pm ws ping                                    # connectivity check
pm ws book <TOKEN>                            # one-shot book snapshot via WS
pm ws book-watch <TOKEN>                      # stream book updates
pm ws user                                    # stream your order + trade events
pm ws user --markets cond1,cond2              # filter to specific condition ids
```

Connection state survives transient disconnects — the SDK auto-reconnects and replays the subscription.

### Conditional Token Framework

Helpers for the Gnosis CTF protocol the markets settle on. Mixes pure off-chain calculations, a JSON-RPC fallback for the EC-heavy collection-id, and Safe-mode writes through the relayer.

```bash
# Pure off-chain — no RPC, no signer
pm ctf condition-id --oracle 0xUMA --question 0x… --outcomes 2
pm ctf position-id  --collateral 0xUSDW --collection 0x…

# RPC fallback — calls CTF.getCollectionId(parent, condition, indexSet) on-chain
# (the local formula needs alt_bn128 EC point addition, which we defer to the chain).
pm ctf collection-id --network-config examples/networks/monad-hermestrade.yaml \
        --condition-id 0x… --index-set 1

# Safe-mode writes — same path-B flow as `pm approve set`. Default dry-run; --execute submits.
pm ctf redeem --network-config <yaml> --condition-id 0x… --index-sets 1
pm ctf split  --network-config <yaml> --condition-id 0x… --partition 1,2 --amount 1000
pm ctf merge  --network-config <yaml> --condition-id 0x… --partition 1,2 --amount 1000
```

Amounts are in raw smallest units (USDW has 6 decimals, so 1 USDW = `1_000_000`). For `split` / `merge`, ensure the Safe holds enough collateral (split) or a full outcome-token set (merge); `redeem` only succeeds after the condition is reported on-chain.

`split` / `merge` go directly through `ConditionalTokens` — the Safe must have USDW approved for that contract (not in the default `approve set` target list). One-time setup:

```bash
pm approve set --asset usdw --spender 0xd77d550092aB455bd1b9071E4185eCbB6E8d6a2A --execute
```

(Address shown is the Monad ConditionalTokens contract; check your YAML's `contracts.conditional_tokens` value.)

## Common workflows

### Browse markets without a wallet

```bash
pm --tenant hermestrade.xyz gamma events list --limit 5
pm --tenant hermestrade.xyz book 3404...0576
pm --tenant hermestrade.xyz price-history 3404...0576 --interval 1d
```

### From zero to first order

```bash
# 1. Pick wallet + chain config once
pm wallet create --signature-type gnosis-safe --chain-id 143 \
                 --scope-id 0x1811a132...196e95
pm wallet set-safe 0xYOUR_SAFE                  # the Safe controlled by your EOA

# 2. Verify the Safe is funded + check current approval state
pm balance --asset-type collateral
pm approve check --network-config examples/networks/monad-hermestrade.yaml

# 3. If approvals are missing, batch USDW.approve + CTF.setApprovalForAll in
#    ONE Safe meta-tx via the relayer (relayer pays gas, you pay 0 USDW).
pm approve set --network-config examples/networks/monad-hermestrade.yaml --execute

# (Optional) If you plan to use `pm ctf split/merge`, also approve the
# ConditionalTokens contract as a USDW spender:
pm approve set --network-config examples/networks/monad-hermestrade.yaml \
               --asset usdw --spender 0xd77d550092aB455bd1b9071E4185eCbB6E8d6a2A --execute

# 4. Mint an L2 API key for trading
pm auth create-key

# 5. Fire your first order
pm order create --token 3404...0576 --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker 0xYOUR_SAFE
```

### Place + cancel cycle (no fill)

```bash
ID=$(pm order create --token <T> --side buy --price 0.10 --size 5 \
                     --fee-rate-bps 20 --maker <SAFE> -o json | jq -r .orderID)
pm order get $ID
pm order cancel $ID
```

### Cross-spread fill (real trade, real money)

```bash
# Yes book — best ASK 0.09 × 10
ID=$(pm order create --token <YES_TOKEN> --side buy --price 0.09 --size 5 \
                     --fee-rate-bps 20 --maker <SAFE> -o json | jq -r .orderID)
# Order will return with status="matched" and a tradeIDs[] populated.
pm trade
pm balance --asset-type conditional --token <YES_TOKEN>
```

### Monitor your trades over WS

```bash
# Terminal A — start the user channel before placing the order
pm ws user

# Terminal B — fire the order
pm order create ...
# Terminal A prints the matching trade + lifecycle order events as they arrive.
```

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `ORDER_SIZE_TOO_SMALL: limit order requires share >= 5` | Order size below the 5-share minimum. | Increase to ≥ 5, even if the per-share price is low. |
| `size 0.66… has 28 decimals; lot size is 2` | Market `--amount / --price` didn't round to 0.01. | Pick `amount` so `amount / price` is a multiple of 0.01. |
| `unknown variant 'MATCHED' / 'cancelled'` from `pm ws user` | Pre-`60904cc` build. | `git pull && cargo build`. |
| `proxy_wallet` differs between API keys | Server returns the proxy from the first key created with a given scope. | Use `pm wallet set-safe <addr>` manually or filter by `--api-key` in code. |
| TLS handshake panic on startup | rustls 0.23 missing crypto provider. | Already fixed in `ee4eec2`. Pull latest. |
| `/heartbeat` returns empty body | Known minor: server may return `{}` rather than `{status: ok}`. Functional, just visually empty. | — |

## Non-goals

Commands intentionally omitted because the backend doesn't expose the underlying endpoint, or because the equivalent is provided through a different surface:

- **Market browsing** — `markets list / get / sampling-markets / simplified-markets`. Discovery is pushed through Gamma instead (`pm gamma events …`).
- **Polymarket rewards** — `rewards list / earnings / reward-percentages / current-rewards / orders-scoring`. Tenants run their own incentive logic.
- **Notifications + account state** — `notifications / closed-only-mode / account-status / geoblock / neg-risk` (the neg-risk flag is embedded in the `/book` response).
- **`bridge`, `rtds`, `rfq`** — Polymarket-proprietary endpoints not present on this platform.
- **EOA-broadcast `ctf` writes** — Polymarket V1 broadcasts `splitPosition / mergePositions / redeemPositions` directly from the EOA. Only `signatureType=2` (Safe) is supported, so `pm ctf {split,merge,redeem}` instead routes through the `relayer-service` (Safe meta-tx). Same functional outcome, different wire path.
- **`upgrade`** — on the roadmap; not yet shipped.

## Output formats

```bash
pm --tenant ... -o table  ...       # default — human-readable
pm --tenant ... -o json   ...       # machine-readable; pipe through jq
```

Or set `PM_OUTPUT=json` once and forget about it.

## License

MIT — see [`LICENSE`](../LICENSE) at the repo root.
