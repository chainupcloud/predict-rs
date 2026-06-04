---
name: predict
description: >-
  Trade prediction markets (e.g. the hermestrade.xyz tenant) with the
  predict-cli binary: install the CLI, set up a wallet and L2 API key, discover
  markets, read orderbooks, place and cancel limit/market orders, track positions
  and PnL, split/merge/redeem conditional tokens, and stream live WebSocket
  updates. Use this skill whenever the user mentions predict-cli, predict-rs,
  hermestrade, prediction markets, outcome shares, YES/NO tokens,
  CLOB orders, or conditional tokens (CTF) — even if they don't name the tool
  explicitly, and even for read-only questions like "what's the midpoint" or
  "show my positions".
---

# Trading prediction markets with predict-cli

`predict-cli` is the terminal client for a multi-tenant prediction-market
CLOB exchange.
Every command supports `-o json` for machine-readable output — prefer it when you
need to parse results (pipe to `jq`).

## 0. Get the CLI

Run `scripts/ensure-cli.sh` first. It is idempotent: if `predict-cli` is already
on PATH it does nothing; otherwise it installs the latest release via the
official installer (which verifies the sha256 checksum before installing):

```bash
curl -sSfL https://raw.githubusercontent.com/chainupcloud/predict-rs/main/install.sh | sh
```

If you are working inside a `predict-rs` checkout, `cargo build --release` and
`target/release/predict-cli` works too.

## 1. Point at a tenant

The platform is multi-tenant; nothing is hard-coded. One flag (or env var)
selects the deployment — it derives `clob-api.<host>` / `gamma-api.<host>` /
`wss://clob-ws.<host>` automatically:

```bash
predict-cli --tenant hermestrade.xyz ok        # health check
predict-cli --tenant hermestrade.xyz endpoints # show derived URLs + chain id
```

Env vars mirror every global flag: `PM_NETWORK`, `PM_TENANT`, `PM_CLOB_ENDPOINT`,
`PM_GAMMA_ENDPOINT`, `PM_WS_ENDPOINT`, `PM_CHAIN_ID`, `PM_SCOPE_ID`. Export
`PM_TENANT` once instead of repeating `--tenant`. Use `--clob-endpoint <url>`
only for non-canonical hostnames. (There is no env var for the private key — it
comes from `--private-key` or `config.toml`.)

## 2. Wallet & auth (one-time)

For first-time setup prefer the guided wizard — it walks through wallet, tenant,
Safe detection, and L2 API-key creation in one pass:

```bash
predict-cli setup
```

Manual equivalent:

```bash
predict-cli wallet create                 # fresh EOA, stored 0600 in <config-dir>/config.toml
predict-cli wallet import 0xYOURKEY       # or import an existing key
predict-cli auth create-key               # L2 API key (or derive-key to recover an existing one)
predict-cli wallet set-safe 0xSAFE        # persist the funded Safe address
predict-cli wallet show                   # address + Safe + signature type + config source
```

> **Careful with `wallet detect-safe`.** It trusts the server's `proxy_wallet`
> field and **overwrites** the stored Safe address. Live testing has seen that
> field point at an undeployed, unfunded address while the real funded Safe was
> elsewhere. If you use it, cross-check the result before trading: the address
> must hold the USDW balance (`balance --asset-type collateral` should match
> on-chain `USDW.balanceOf(safe)`) and have contract code deployed. When in
> doubt, `wallet set-safe` the known-funded address instead.

Key facts that prevent confusion later:

- **Default signature type is `gnosis-safe`**: the EOA only signs; a 1-of-1
  Safe holds the USDW and outcome tokens and is the order `maker`. Balances and
  positions belong to the **Safe address**, not the EOA.
- Config lives in `~/.config/pm/config.toml` (Linux) or
  `~/Library/Application Support/pm` (macOS).
- Prefer storing the key in `config.toml` (via `wallet create` / `import` / `setup`)
  over `--private-key` — the flag leaks into shell history and process args. There
  is no `PM_PRIVATE_KEY` env var (a key in the environment leaks via `/proc`).

Run `scripts/preflight.sh [tenant]` before a trading session: it checks server
health, endpoint resolution, wallet config, and collateral balance in one shot.

## 3. Safety rules (real funds)

Orders and CTF operations move real money. Hold to these:

- **Confirm before committing funds.** Before any `order create` / `order market`
  / `ctf … --execute` / `approve set --execute`, state the market, side, price,
  size, and resulting notional, and get the operator's explicit go-ahead —
  unless they have already given you a standing budget and instruction.
- **Dry-run first on new flows.** `order create --dry-run` prints the signed
  envelope without posting; `ctf`/`approve` writes default to dry-run and only
  submit with `--execute`. Inspect, then re-run for real.
- **Never print private keys.** `wallet show` is safe (it never echoes the key);
  `config.toml` contents are not — don't cat it.
- **Stay inside any budget the operator set**, and stop and report rather than
  retry when a money-moving call fails in an unexpected way.

## 4. Discover markets

```bash
predict-cli gamma search "fed rate cuts" --limit-per-type 5
predict-cli gamma events list --limit 10
predict-cli gamma events get <slug>                 # e.g. how-many-fed-rate-cuts-in-2026-pm-406282
predict-cli gamma markets get <slug-or-condition-id>
```

From a market object you need two identifiers:

- `conditionId` (`0x…` hex) — keys the market for `order cancel-market`,
  `ws user --market`, and CTF operations.
- `clobTokenIds` — the YES/NO outcome token ids (uint256 decimals). Orders,
  books, and prices are all per **token id**.

## 5. Read the market

`scripts/market-snapshot.sh <TOKEN_ID> [tenant]` prints tick size, fee rate,
midpoint, spread, last trade, and the order book in one call — run it before
quoting a price. Individual reads:

```bash
predict-cli tick-size <TOKEN_ID>     # price granularity — orders must respect it
predict-cli fee-rate <TOKEN_ID>      # feeRateBps — required on every order
predict-cli midpoint <TOKEN_ID>
predict-cli book <TOKEN_ID>
predict-cli price <TOKEN_ID> --side buy
```

Batch variants (`midpoints` / `prices` / `spreads` / `books` / `last-trades`)
accept comma-separated ids, ≤ 500 per call.

## 6. Place orders

Always fetch `fee-rate` and `tick-size` for the token first; the server rejects
orders that miss the fee or violate price granularity.

**Pick the right exchange contract.** The platform runs two exchanges and the
order signature embeds the exchange address (EIP-712 `verifyingContract`):
standalone binary markets settle on the **CTF Exchange**, while sports /
multi-outcome families settle on the **Neg Risk CTF Exchange** (addresses in
the built-in network registry / `gamma public-info`). An order signed against the wrong one
is rejected with `EXECUTION_ERROR: INVALID_SIGNATURE: signer mismatch` — if you
hit that with a correct key and maker, re-sign with the other exchange via
`--exchange-address <addr>` (or `PM_EXCHANGE_ADDRESS`).

Limit order (default Safe mode — note `--maker` is **required** and is the Safe
address from `wallet show`; it is *not* auto-filled from config):

```bash
predict-cli order create \
  --token <TOKEN_ID> --side buy --price 0.34 --size 100 \
  --fee-rate-bps <FROM_FEE_RATE> \
  --maker <SAFE_ADDRESS> \
  --dry-run          # drop after inspecting the envelope
```

Market order (FAK by default; `--amount` is USDC notional, BUY only;
`--size` is share-denominated):

```bash
predict-cli order market --token <TOKEN_ID> --side buy --amount 25 --fee-rate-bps <BPS> --maker <SAFE>
```

Rules the exchange enforces:

- `price` ∈ (0, 1), decimals capped by tick size (tick 0.01 → 2 dp, 0.001 → 3 dp,
  0.0001 → 4 dp). `size` max 2 decimals; amounts floor-truncate to 6 decimals.
- Per-event **minimum order size** — a too-small order is rejected server-side.
- Order types: `gtc` (default limit), `gtd` (requires `--expiration` unix-seconds),
  `fok` / `fak` (market). `--post-only` makes a limit order maker-only.
- EOA mode (`--signature-type eoa`): `--maker` defaults to the signer address.

Cancel / inspect:

```bash
predict-cli order list                       # open orders
predict-cli order get <ORDER_ID>
predict-cli order cancel <ORDER_ID>
predict-cli order cancel-many id1,id2,id3    # ≤ 3000
predict-cli order cancel-market --market 0xCONDITION_ID   # and/or --asset-id <TOKEN_ID>
predict-cli order cancel-all                 # everything for the API key — confirm first
predict-cli order replace --cancel id1 --orders-file new.json   # new.json from --dry-run output
predict-cli order post-batch …               # ≤ 15 orders, shared side/fee/maker
```

## 7. Track fills and balances

```bash
predict-cli trade --asset-id <TOKEN_ID> --limit 50   # trade history (L2-auth)
predict-cli balance --asset-type collateral          # USDW
predict-cli balance --asset-type conditional --token <TOKEN_ID>
```

(`balance --update` currently fails against the live backend — the
`/balance-allowance/update` endpoint returns an empty body the SDK can't
decode. Plain `balance` reads are accurate; cross-check on-chain when it
matters.)

A trade settles in stages — on `/ws/user` (and in `trade` output) the status
walks `MATCHED → MINED → CONFIRMED` (UPPERCASE; `RETRYING` on relayer retry;
fast settlement may skip `MINED` entirely). Treat a fill as final only at
`CONFIRMED`; `MATCHED` means the engine matched it but on-chain settlement is
still pending. Two wire quirks to expect on `/ws/user` trade events: the
`MATCHED` frame can carry the *complement side's* price under `match_type:
"MINT"` (full-set mint against your order), and later frames may switch
`size`/`price` to raw 6-decimal units (`5000000` = 5 shares). Fees on BUY are
taken **in shares** (you receive slightly less than `size`); maker-program
rebates show up as `MAKER_REBATE` rows in `data activity`.

## 8. Positions & PnL

The Data API is keyed by **wallet address — use the Safe address**:

```bash
predict-cli data positions <SAFE_ADDRESS>
predict-cli data closed-positions <SAFE_ADDRESS>
predict-cli data trades <SAFE_ADDRESS>
predict-cli data activity <SAFE_ADDRESS>     # trades + splits + merges + redeems
predict-cli data user-pnl <SAFE_ADDRESS>
```

## 9. CTF operations (split / merge / redeem)

On-chain writes go through the relayer as Safe meta-transactions. They run
against the selected built-in network (`--network <name>`, default `monad`),
whose registry supplies chain id, RPC, and contract addresses, and default to
**dry-run**:

```bash
# One-time prerequisite: USDW allowance + CTF setApprovalForAll
predict-cli approve check
predict-cli approve set   --execute

# --amount is RAW 6-decimal units: 1000000 = 1 USDW. Run without --execute
# first and read back the dry-run plan's amount before submitting.
predict-cli ctf split  --condition-id 0x… --partition 1,2 --amount 1000000 --execute
predict-cli ctf merge  --condition-id 0x… --partition 1,2 --amount 1000000 --execute
predict-cli ctf redeem --condition-id 0x… --index-sets 1,2 --execute   # only after resolution
```

`redeem` succeeds only once the condition is resolved on-chain (non-zero
`payoutNumerators`). Pure helpers `ctf condition-id` / `position-id` /
`collection-id` compute identifiers without submitting anything.

## 10. Watch live

```bash
predict-cli ws ping                                   # connectivity check
predict-cli ws book <TOKEN_ID> --count 5              # N frames, then exit
predict-cli ws book-watch <TOKEN_ID>                  # stream until Ctrl-C (--print-as-json for jq)
predict-cli ws user --market <CONDITION_ID>           # own orders + trades (auto-derives L2 creds)
```

For one-shot checks prefer REST reads; use `ws` when the user wants continuous
monitoring or to wait for a fill.

## 11. Troubleshooting

| Symptom | Fix |
|---------|-----|
| `no private key configured` | `predict-cli wallet create` / `import` (stores it in `config.toml`), or pass `--private-key` |
| 401 / `authentication failed` | `predict-cli auth derive-key` (existing key) or `auth create-key` |
| `--maker is required for signature_type=gnosis-safe` | pass the Safe address from `wallet show` (use `set-safe` to persist; treat `detect-safe` output as untrusted — see §2) |
| `INVALID_SIGNATURE: signer mismatch` on `POST /order` | wrong exchange for this market — re-sign with the other one via `--exchange-address` (CTF vs Neg Risk, see §6) |
| price rejected | re-check `tick-size` — too many decimals for this market |
| order below minimum | raise size; the per-event minimum is server-enforced |
| allowance / transfer failures on split or first order | `approve check`, then `approve set --execute` |
| `next_cursor: "LTE="` in paginated output | end of stream — stop paging |

Deeper reference: `docs/orders.md`, `docs/ws.md`, `docs/wallet.md`,
`docs/auth-flow.md`, `docs/gamma.md` in the
[predict-rs repo](https://github.com/chainupcloud/predict-rs).
