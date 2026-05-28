# CLAUDE.md — pm-rs

## Language policy

**All content in this repository must be in English.** This includes:

- All documentation (`README.md`, `CLAUDE.md`, files under `docs/`)
- All code comments and doc-comments
- All error-message strings and log messages
- All git commit messages and PR descriptions

Telegram chat replies to the user may remain Chinese; this rule applies to repository content only.

## Project scope

`pm-rs` is the Rust toolchain for [`pm-cup2026`](https://github.com/chainupcloud/pm-cup2026) prediction-market platform. Cargo workspace with two member crates:

- `pm-rs-clob-client` (lib) — CLOB / Gamma / WebSocket SDK. Counterpart of [`pm-sdk-go`](https://github.com/chainupcloud/pm-sdk-go); ported from Polymarket's `rs-clob-client` V1 with platform-specific extensions.
- `pm-cli` (bin: `pm`) — terminal client. Counterpart of Polymarket's `polymarket-cli`.

## Hard constraints (drive API design)

### 1. Multi-tenant SaaS

`pm-cup2026` is a **multi-tenant SaaS** platform:

- Each tenant has its own **dedicated domain** (e.g. `clob-api.tenant-a.example.com`), distinguished by the HTTP `Host` header.
- Multiple tenants may **share a unified order book** (same backend, different `scopeId`).
- `scopeId` (`bytes32`) is the multi-tenant isolation field embedded in the EIP-712 `ClobAuth` and `Order` structs.

**Implications for the SDK:**

- ❌ **Do not** hard-code endpoint hosts anywhere in the code.
- ✅ `ClientBuilder::endpoint(...)` is mandatory or read explicitly from config / CLI flag / env.
- ✅ `scope_id` is a first-class field on [`PMCup26Signer`] (default zero), decoupled from the endpoint.
- ✅ The CLI supports `--endpoint` and `--scope-id` flags plus matching env vars.

### 2. Configurable multi-chain

`pm-cup2026` is **chain-agnostic**; the target network is confirmed at deploy time. Currently in scope:

| Network | chainID | RPC | Gas Token | Status |
|---------|---------|-----|-----------|--------|
| OP Sepolia | 11155420 | `https://api.zan.top/opt-sepolia` | ETH | dev / staging |
| Monad | (TBD) | (TBD) | MON | planned |

Additional EVM networks will be added in the future.

**Implications for the SDK:**

- ❌ **Do not** hard-code `chain_id`, RPC URL, or contract addresses (exchange / collateral / CTF / fee module).
- ❌ **Do not** put chain-related constants in a `phf_map!` inside `lib.rs` (the pattern used by Polymarket's `rs-clob-client/src/lib.rs` — **do not copy that approach**).
- ✅ `chain_id`, `exchange_address`, RPC URL, gas-token symbol all come from **runtime configuration**.
- ✅ The CLI config file supports multiple `[networks.<name>]` sections with a `--network <name>` flag to switch.
- ✅ Any method that interacts with chain-level constants takes them as explicit parameters; nothing is read from a global.
- ✅ Reference network YAMLs live under `examples/networks/` (one file per network, e.g. `monad-hermestrade.yaml`).

### 3. Behavioral parity with pm-sdk-go

The byte-level output of every signing primitive (`ClobAuth`, `Order`) **must** match `pm-sdk-go/pkg/signer`. Any change to the signer requires re-running:

```bash
cargo test -p pm-rs-clob-client --test golden_signer
```

Golden vectors live at `clob-client/tests/fixtures/golden.json` (a copy of `pm-sdk-go/pkg/signer/testdata/golden.json`). If the upstream Go fixture changes, sync the file here and re-run the test.

### 4. Wire-format differences vs Polymarket

- Authentication headers use the `PRED_*` prefix (Polymarket uses `POLY_*`).
- HMAC secret uses **standard** base64 (Polymarket uses URL-safe).
- `ClobAuth` is a **5-field** struct with `bytes32 scopeId` inserted between `nonce` and `message`.
- `Order` is a **13-field** struct with `bytes32 scopeId` appended at the end.
- The `ClobAuth` EIP-712 domain is **short-form** — no `verifyingContract`.
- The `Order` EIP-712 domain name is `"Prediction Market Protocol"`, **not** `"Polymarket CTF Exchange"`.

Full diff table: [`docs/diff-vs-polymarket-v1.md`](docs/diff-vs-polymarket-v1.md).

## Phased delivery

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Workspace skeleton + signer (golden test passes) + public CLOB REST surface + 9 read-only CLI subcommands | Done |
| 2 | L1 / L2 auth + create / cancel order + balance / orders / trades | Done |
| 3 | Gamma client + WebSocket subscriptions + interactive shell | Done |
| 4 | Safe meta-tx writes via relayer: approvals, CTF split / merge / redeem | Done |

## Development conventions

- **No filler comments.** Only document constants or fields whose meaning is non-obvious from the name.
- **Test-driven for the signer.** Golden tests gate every signing change; do not stack business logic on a signer that hasn't been re-verified.
- **Error handling.** Crate-level `Error` enum built on `thiserror`; `Result<T> = Result<T, Error>`.
- **Git workflow.** Day-to-day changes land on `dev` branch and merge to `main` via PR. The initial repo bootstrap commit went directly to `main`.
