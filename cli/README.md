# predict-cli (`predict-cli`)

Terminal client for the prediction market platform. Browse markets, place orders, manage positions — with feature parity for everything the backend exposes (see [Non-goals](#non-goals) for what it deliberately skips).

```bash
$ predict-cli --tenant hermestrade.xyz time
2026-05-20T02:25:10Z

$ predict-cli --tenant hermestrade.xyz book 3404...0576
asks:
  0.75 × 5
bids:
  0.68 × 10
```

## Install

### Build from source

```bash
git clone https://github.com/chainupcloud/predict-rs.git
cd predict-rs
cargo build --release
install -m 0755 target/release/predict-cli ~/.local/bin/predict-cli
```

Requires Rust 1.88+ (pinned via `rust-version` in the workspace `Cargo.toml`).

## Quick start

### Read-only — no wallet needed

```bash
# Point at a tenant — clob-api / gamma-api / clob-ws are derived automatically.
predict-cli --tenant hermestrade.xyz ok                  # server health
predict-cli --tenant hermestrade.xyz time
predict-cli --tenant hermestrade.xyz endpoints           # show derived URLs + chain id
predict-cli --tenant hermestrade.xyz book   <TOKEN_ID>
predict-cli --tenant hermestrade.xyz midpoint <TOKEN_ID>
predict-cli --tenant hermestrade.xyz gamma events get how-many-fed-rate-cuts-in-2026-pm-406282
```

Or supply the CLOB URL directly (useful for non-canonical hostnames or local dev):

```bash
predict-cli --clob-endpoint https://clob-api.hermestrade.xyz time
```

JSON for scripts:

```bash
predict-cli --tenant hermestrade.xyz -o json book 3404...0576 | jq '.bids[0]'
```

### Trading — wallet + L2 credentials

```bash
# 1. Wallet — pick one
predict-cli wallet create                                # generates a fresh EOA, stores 0600
predict-cli wallet import 0xYOURKEY                      # or import an existing one
predict-cli wallet set-safe 0xYOUR_SAFE_ADDRESS          # required when signature-type = gnosis-safe
predict-cli wallet show                                  # eoa + safe + source

# 2. Create an L2 API key (writes credentials.json mode 0600)
predict-cli auth create-key --output json > credentials.json

# 3. Trade
export PM_TENANT=hermestrade.xyz
export PM_CHAIN_ID=143
export PM_SCOPE_ID=0x1811a132dd725e2c40475aa52df39025b36544f7a70825968e32b28da2196e95
export PM_CREDENTIALS_FILE=$PWD/credentials.json

predict-cli balance --asset-type collateral
predict-cli order create --token 3404...0576 --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker 0xYOUR_SAFE
predict-cli order list
predict-cli order cancel <ORDER_ID>
```

The first run prompts auto-pick a sensible config dir (`~/.config/pm` on Linux, mode 0700). Override with `--config-dir` or `PM_CONFIG_DIR`.

## Configuration

### Resolution order

Every connection flag (`--tenant`, `--clob-endpoint`, `--chain-id`, `--scope-id`, `--private-key`, …) resolves in this order:

1. CLI flag — wins.
2. Env var — `PM_TENANT`, `PM_CLOB_ENDPOINT`, `PM_CHAIN_ID`, `PM_SCOPE_ID`, `PM_PRIVATE_KEY`, `PM_SIGNATURE_TYPE`, `PM_EXCHANGE_ADDRESS`, `PM_CONFIG_DIR`, `PM_CREDENTIALS_FILE`, `PM_OUTPUT`.
3. Stored config — `<config-dir>/config.toml` (written by `predict-cli wallet …`).

Empty values are treated as unset.

### Signature types

| Value | Type | Use when |
|-------|------|----------|
| `eoa` | 0 — direct EOA signing | Funds held in the same EOA that signs. |
| `proxy` | 1 — upstream V1 proxy wallet | Legacy / interop. |
| `gnosis-safe` (**default**) | 2 — 1-of-1 Gnosis Safe | **Default.** EOA signs; the Safe is the `maker` and holds the funds. |

The default is `gnosis-safe`. Persist a different choice with `predict-cli wallet create --signature-type eoa`, or override per-invocation via `--signature-type <eoa|proxy|gnosis-safe>` / `PM_SIGNATURE_TYPE`.

### `scopeId` — multi-tenant isolation

`scopeId` is a `bytes32` value embedded in every signed `ClobAuth` and `Order`. Two clients on the same EOA but different scopes derive different L2 keys and never share order state. Fetch the right one with:

```bash
# From the server (returns the canonical scope for your tenant)
curl https://clob-api.<tenant>/auth/nonce | jq -r .scopeId

# Or via the CLI
predict-cli auth nonce | grep scopeId
```

Set it via flag, env var, or `predict-cli wallet create --scope-id 0x…`.

### Network config (`approve check`, `approve set`, `ctf …`)

Every command that touches the chain (`predict-cli approve check / set`, `predict-cli ctf redeem / split / merge / collection-id`) needs a tenant network YAML. One ships at [`examples/networks/monad-hermestrade.yaml`](../examples/networks/monad-hermestrade.yaml):

```bash
predict-cli approve check --network-config examples/networks/monad-hermestrade.yaml
predict-cli approve set   --network-config examples/networks/monad-hermestrade.yaml --execute
predict-cli ctf split     --network-config examples/networks/monad-hermestrade.yaml --condition-id 0x… --partition 1,2 --amount 1000000 --execute   # amount = raw 6-decimal units (1000000 = 1 USDW)
```

The YAML is the single source of truth for chain id, RPC URL, contract addresses (USDW, CTF, exchanges), and the relayer endpoint. It's the same shape the backend deploy tooling uses.

## Commands

### Market data — public, no auth

| Command | What it does |
|---------|--------------|
| `predict-cli ok` / `predict-cli time` | Server health + clock |
| `predict-cli endpoints` | Show the derived clob / gamma / ws URLs + chain id |
| `predict-cli midpoint <TOKEN>` | Single-token midpoint |
| `predict-cli price <TOKEN> --side buy` | Last price (one side) |
| `predict-cli spread <TOKEN>` | Best-bid / best-ask + spread |
| `predict-cli book <TOKEN>` | Top-of-book depth |
| `predict-cli tick-size <TOKEN>` | Active tick size |
| `predict-cli fee-rate <TOKEN>` | Fee rate bps |
| `predict-cli last-trade <TOKEN>` | Last trade price |
| `predict-cli price-history <TOKEN> --interval 1h \| 6h \| 1d \| 1w \| 1m \| all` | Historical price points |
| `predict-cli midpoints t1 t2 ...` | Batch (≤ 500 tokens) |
| `predict-cli prices t1:buy t2:sell ...` | Batch — per-token side selectable |
| `predict-cli spreads t1 t2 ...` | Batch spreads |
| `predict-cli books t1:buy t2:sell ...` | Batch books |
| `predict-cli last-trades t1 t2 ...` | Batch last trades |

### Gamma — event / market discovery

```bash
predict-cli gamma events list --limit 10
predict-cli gamma events get how-many-fed-rate-cuts-in-2026-pm-406282
predict-cli gamma events tags 291
predict-cli gamma markets get <CONDITION_ID>            # or slug
predict-cli gamma profiles get <SAFE_ADDRESS>
predict-cli gamma tags list
```

Gamma is REST-only; there is no streaming variant.

### Wallet

```bash
predict-cli wallet create [--force]                    # random EOA, mode 0600
predict-cli wallet import 0xHEXKEY
predict-cli wallet address                             # print EOA only
predict-cli wallet show                                # eoa + safe + source
predict-cli wallet reset                               # delete config
predict-cli wallet set-safe 0xSAFE                     # store Safe address (gnosis-safe mode)
predict-cli wallet detect-safe                         # ask the server for the Safe linked to the API key
```

### Authentication (L1 + L2 API keys)

```bash
predict-cli auth nonce                                 # nonce + scopeId for the current EOA
predict-cli auth derive-key                            # deterministic L2 key derivation (no server write)
predict-cli auth create-key                            # POST /auth/api-key
predict-cli auth list-keys
predict-cli auth delete-key <UUID> [--nonce N]
```

### Trading

```bash
# Place a limit order (default GTC)
predict-cli order create --token <T> --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker <SAFE>

# postOnly / GTD
predict-cli order create --token <T> --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker <SAFE> --post-only
predict-cli order create --token <T> --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker <SAFE> \
                --order-type gtd --expiration $(( $(date +%s) + 600 ))

# Market order (BUY only — amount denominated in USDW; server runs the book walk)
predict-cli order create --token <T> --side buy --amount 3.75 --price 0.75 \
                --fee-rate-bps 20 --maker <SAFE> --market

# Batch place
predict-cli order post-batch --tokens t1,t2 --prices 0.10,0.05 --sizes 5,5 \
                    --side buy --fee-rate-bps 20 --maker <SAFE>

# Manage
predict-cli order list
predict-cli order get <ID>
predict-cli order cancel <ID>
predict-cli order cancel-many <ID1>,<ID2>,...
predict-cli order cancel-all
predict-cli order replace --orders-file replace.json   # atomic cancel + re-place

# Dry-run anywhere — prints the signed envelope, does NOT post
predict-cli order create ... --dry-run -o json
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
predict-cli trade                                      # your trade history
predict-cli trade --token <T>
predict-cli balance --asset-type collateral
predict-cli balance --asset-type conditional --token <T>
predict-cli balance --update                           # force refresh from the server
predict-cli fee-rate                                   # account-level fee tier
predict-cli heartbeat                                  # server-side liveness ping
```

### Approval helpers

```bash
# Read-only — query allowance / setApprovalForAll status for every YAML target.
predict-cli approve check --network-config examples/networks/monad-hermestrade.yaml

# Write — issue approvals via Safe meta-tx through the relayer.
# Defaults to dry-run (signs locally + prints the SubmitRequest, never POSTs).
predict-cli approve set --network-config examples/networks/monad-hermestrade.yaml

# Default `--asset all` batches USDW.approve(target, MAX) +
# CTF.setApprovalForAll(target, true) for every approval target into one
# MultiSend. This is what a fresh community wallet needs.
predict-cli approve set --network-config examples/networks/monad-hermestrade.yaml --execute

# Narrow the batch:
predict-cli approve set --asset usdw --execute              # USDW.approve only
predict-cli approve set --asset ctf  --execute              # CTF.setApprovalForAll only
predict-cli approve set --spender 0x017641…  --execute      # one target only (single Call, not MultiSend)
predict-cli approve set --spender 0xd77d5500…  --execute    # add ConditionalTokens — prerequisite for predict-cli ctf split/merge
```

Gas is paid by the relayer's key pool; the user spends **zero collateral**. Polling is built-in: `--poll-interval-secs` (default 2) and `--poll-timeout-secs` (default 60) control how long the CLI waits for `STATE_CONFIRMED`.

### Safe-mode writes via the relayer (path B)

Every `predict-cli` write command runs through the same flow — the only difference between `approve set` and `ctf {redeem,split,merge}` is the encoded calldata:

1. **JWT login** — `Client::jwt_login` hits gamma-service `/auth/nonce` → signs an EIP-712 `LoginMessage` → `POST /auth/login` → returns a Bearer JWT.
2. **Safe nonce** — read `Safe.nonce()` from the YAML's `network.rpc_url`.
3. **Build SafeTx** — either a single `Call` (one op) or `DelegateCall` to MultiSend (N ops).
4. **Sign** — `PMCup26Signer::sign_safe_tx` produces 65 bytes with Ethereum `v` in {0x1b, 0x1c}.
5. **Submit** — `POST relayer /submit` with the signed `SubmitRequest`. Returns a `transactionID` immediately; the relayer broadcasts asynchronously.
6. **Poll** — `GET relayer /transaction?id=…` until terminal: `STATE_CONFIRMED`, `STATE_FAILED`, or `STATE_DROPPED`. CLI surfaces the final tx hash + state.

You don't pay gas (the relayer covers it from its own key pool). You don't need any external broadcaster. All you need is the EOA private key + the Safe address.

### WebSocket

```bash
predict-cli ws ping                                    # connectivity check
predict-cli ws book <TOKEN>                            # one-shot book snapshot via WS
predict-cli ws book-watch <TOKEN>                      # stream book updates
predict-cli ws user                                    # stream your order + trade events
predict-cli ws user --markets cond1,cond2              # filter to specific condition ids
```

Connection state survives transient disconnects — the SDK auto-reconnects and replays the subscription.

### Conditional Token Framework

Helpers for the Gnosis CTF protocol the markets settle on. Mixes pure off-chain calculations, a JSON-RPC fallback for the EC-heavy collection-id, and Safe-mode writes through the relayer.

```bash
# Pure off-chain — no RPC, no signer
predict-cli ctf condition-id --oracle 0xUMA --question 0x… --outcomes 2
predict-cli ctf position-id  --collateral 0xUSDW --collection 0x…

# RPC fallback — calls CTF.getCollectionId(parent, condition, indexSet) on-chain
# (the local formula needs alt_bn128 EC point addition, which we defer to the chain).
predict-cli ctf collection-id --network-config examples/networks/monad-hermestrade.yaml \
        --condition-id 0x… --index-set 1

# Safe-mode writes — same path-B flow as `predict-cli approve set`. Default dry-run; --execute submits.
predict-cli ctf redeem --network-config <yaml> --condition-id 0x… --index-sets 1
predict-cli ctf split  --network-config <yaml> --condition-id 0x… --partition 1,2 --amount 1000000   # raw 6-decimal units
predict-cli ctf merge  --network-config <yaml> --condition-id 0x… --partition 1,2 --amount 1000000   # raw 6-decimal units
```

Amounts are in raw smallest units (USDW has 6 decimals, so 1 USDW = `1_000_000`). For `split` / `merge`, ensure the Safe holds enough collateral (split) or a full outcome-token set (merge); `redeem` only succeeds after the condition is reported on-chain.

`split` / `merge` go directly through `ConditionalTokens` — the Safe must have USDW approved for that contract (not in the default `approve set` target list). One-time setup:

```bash
predict-cli approve set --asset usdw --spender 0xd77d550092aB455bd1b9071E4185eCbB6E8d6a2A --execute
```

(Address shown is the Monad ConditionalTokens contract; check your YAML's `contracts.conditional_tokens` value.)

## Common workflows

### Browse markets without a wallet

```bash
predict-cli --tenant hermestrade.xyz gamma events list --limit 5
predict-cli --tenant hermestrade.xyz book 3404...0576
predict-cli --tenant hermestrade.xyz price-history 3404...0576 --interval 1d
```

### From zero to first order

```bash
# 1. Pick wallet + chain config once
predict-cli wallet create --signature-type gnosis-safe --chain-id 143 \
                 --scope-id 0x1811a132...196e95
predict-cli wallet set-safe 0xYOUR_SAFE                  # the Safe controlled by your EOA

# 2. Verify the Safe is funded + check current approval state
predict-cli balance --asset-type collateral
predict-cli approve check --network-config examples/networks/monad-hermestrade.yaml

# 3. If approvals are missing, batch USDW.approve + CTF.setApprovalForAll in
#    ONE Safe meta-tx via the relayer (relayer pays gas, you pay 0 USDW).
predict-cli approve set --network-config examples/networks/monad-hermestrade.yaml --execute

# (Optional) If you plan to use `predict-cli ctf split/merge`, also approve the
# ConditionalTokens contract as a USDW spender:
predict-cli approve set --network-config examples/networks/monad-hermestrade.yaml \
               --asset usdw --spender 0xd77d550092aB455bd1b9071E4185eCbB6E8d6a2A --execute

# 4. Mint an L2 API key for trading
predict-cli auth create-key

# 5. Fire your first order
predict-cli order create --token 3404...0576 --side buy --price 0.10 --size 5 \
                --fee-rate-bps 20 --maker 0xYOUR_SAFE
```

### Place + cancel cycle (no fill)

```bash
ID=$(predict-cli order create --token <T> --side buy --price 0.10 --size 5 \
                     --fee-rate-bps 20 --maker <SAFE> -o json | jq -r .orderID)
predict-cli order get $ID
predict-cli order cancel $ID
```

### Cross-spread fill (real trade, real money)

```bash
# Yes book — best ASK 0.09 × 10
ID=$(predict-cli order create --token <YES_TOKEN> --side buy --price 0.09 --size 5 \
                     --fee-rate-bps 20 --maker <SAFE> -o json | jq -r .orderID)
# Order will return with status="matched" and a tradeIDs[] populated.
predict-cli trade
predict-cli balance --asset-type conditional --token <YES_TOKEN>
```

### Monitor your trades over WS

```bash
# Terminal A — start the user channel before placing the order
predict-cli ws user

# Terminal B — fire the order
predict-cli order create ...
# Terminal A prints the matching trade + lifecycle order events as they arrive.
```

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `ORDER_SIZE_TOO_SMALL: limit order requires share >= 5` | Order size below the 5-share minimum. | Increase to ≥ 5, even if the per-share price is low. |
| `size 0.66… has 28 decimals; lot size is 2` | Market `--amount / --price` didn't round to 0.01. | Pick `amount` so `amount / price` is a multiple of 0.01. |
| `unknown variant 'MATCHED' / 'cancelled'` from `predict-cli ws user` | Pre-`60904cc` build. | `git pull && cargo build`. |
| `proxy_wallet` differs between API keys | Server returns the proxy from the first key created with a given scope. | Use `predict-cli wallet set-safe <addr>` manually or filter by `--api-key` in code. |
| TLS handshake panic on startup | rustls 0.23 missing crypto provider. | Already fixed in `ee4eec2`. Pull latest. |
| `/heartbeat` returns empty body | Known minor: server may return `{}` rather than `{status: ok}`. Functional, just visually empty. | — |

## Non-goals

Commands intentionally omitted because the backend doesn't expose the underlying endpoint, or because the equivalent is provided through a different surface:

- **Market browsing** — `markets list / get / sampling-markets / simplified-markets`. Discovery is pushed through Gamma instead (`predict-cli gamma events …`).
- **Upstream V1 rewards** — `rewards list / earnings / reward-percentages / current-rewards / orders-scoring`. Tenants run their own incentive logic.
- **Notifications + account state** — `notifications / closed-only-mode / account-status / geoblock / neg-risk` (the neg-risk flag is embedded in the `/book` response).
- **`bridge`, `rtds`, `rfq`** — upstream V1-proprietary endpoints not present on this platform.
- **EOA-broadcast `ctf` writes** — upstream V1 broadcasts `splitPosition / mergePositions / redeemPositions` directly from the EOA. Only `signatureType=2` (Safe) is supported, so `predict-cli ctf {split,merge,redeem}` instead routes through the `relayer-service` (Safe meta-tx). Same functional outcome, different wire path.
- **`upgrade`** — on the roadmap; not yet shipped.

## Output formats

```bash
predict-cli --tenant ... -o table  ...       # default — human-readable
predict-cli --tenant ... -o json   ...       # machine-readable; pipe through jq
```

Or set `PM_OUTPUT=json` once and forget about it.

## License

MIT — see [`LICENSE`](../LICENSE) at the repo root.
