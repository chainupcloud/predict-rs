# CLAUDE.md — predict-rs

## Language policy

**All content in this repository must be in English.** This includes:

- All documentation (`README.md`, `CLAUDE.md`, files under `docs/`)
- All code comments and doc-comments
- All error-message strings and log messages
- All git commit messages and PR descriptions

Telegram chat replies to the user may remain Chinese; this rule applies to repository content only.

## Project scope

`predict-rs` is the Rust toolchain for the prediction market platform. Cargo workspace with two member crates:

- `predict-rs-clob-client` (lib) — CLOB / Gamma / WebSocket SDK. Counterpart of `pm-sdk-go`; ported from the upstream V1 `rs-clob-client` with platform-specific extensions.
- `predict-cli` (bin: `predict-cli`) — terminal client. Counterpart of the upstream V1 CLI.

## Hard constraints (drive API design)

### 1. Multi-tenant SaaS

The prediction market platform is **multi-tenant SaaS**:

- Each tenant has its own **dedicated domain** (e.g. `clob-api.hermestrade.xyz`), distinguished by the HTTP `Host` header.
- Multiple tenants may **share a unified order book** (same backend, different `scopeId`).
- `scopeId` (`bytes32`) is the multi-tenant isolation field embedded in the EIP-712 `ClobAuth` and `Order` structs.

**Implications for the SDK (`clob-client`):**

- ❌ **Do not** hard-code endpoint hosts anywhere in the SDK. Endpoints are always caller-supplied via `ClientBuilder::endpoints(...)` / `tenant(...)`.
- ✅ `scope_id` is a first-class field on [`PMCup26Signer`] (default zero), decoupled from the endpoint.

**Implications for the CLI (`predict-cli`):**

- ✅ The CLI ships a **built-in network registry** (`cli/src/networks/`, each network's definition baked into the binary via `include_str!`), selected with `--network <name>` / `PM_NETWORK` (default `monad`). The selection supplies the tenant domain and endpoints.
- ✅ `--tenant` overrides only the host; precedence is `--tenant` flag / `PM_TENANT` > `config.toml` `tenant` > the selected network's domain. This is the sanctioned way to point the same network at a different tenant (shared order book, different `Host`).
- ✅ `--scope-id` flag + `PM_SCOPE_ID` env + `config.toml` `scope_id` all feed the signer.

### 2. Configurable multi-chain

The platform is **chain-agnostic**; the target network is confirmed at deploy time. Currently in scope:

| Network | chainID | RPC | Gas Token | Status |
|---------|---------|-----|-----------|--------|
| Monad | 143 | `https://rpc.monad.xyz` | MON | built-in (`--network monad`, default) |
| OP Sepolia | 11155420 | `https://api.zan.top/opt-sepolia` | ETH | dev / staging |

Additional EVM networks will be added in the future.

**Implications for the SDK (`clob-client`):**

- ❌ **Do not** hard-code `chain_id`, RPC URL, or contract addresses (exchange / collateral / CTF / fee module) in the SDK. Every such value is a caller-supplied parameter; nothing is read from a global.
- ❌ **Do not** put chain-related constants in a `phf_map!` inside the SDK's `lib.rs` (the upstream V1 `rs-clob-client/src/lib.rs` pattern — **do not copy it**).

**Implications for the CLI (`predict-cli`):**

- ✅ The CLI's **built-in network registry** is the sanctioned home for `chain_id`, RPC URL, gas-token, endpoints, and all contract addresses. Each network is one YAML document under `cli/src/networks/<name>.yaml` (decoded by `cli/src/network_config.rs`); `cli/src/networks.rs` maps `--network <name>` (default `monad`, overridable via `config.toml` `network`) to it. There is **no runtime network YAML** — the old `--network-config <path>` flag is gone.
- ✅ When no flag/config value is given, `chain_id` and `exchange_address` fall back to the selected network. `--chain-id` / `--exchange-address` / `--rpc-url` still override per-invocation.
- ✅ Add a network by dropping a YAML under `cli/src/networks/` and adding a match arm in `cli/src/networks.rs`.

### 3. Behavioral parity with pm-sdk-go

The byte-level output of every signing primitive (`ClobAuth`, `Order`) **must** match `pm-sdk-go/pkg/signer`. Any change to the signer requires re-running:

```bash
cargo test -p predict-rs-clob-client --test golden_signer
```

Golden vectors live at `clob-client/tests/fixtures/golden.json` (a copy of `pm-sdk-go/pkg/signer/testdata/golden.json`). If the upstream Go fixture changes, sync the file here and re-run the test.

### 4. Wire-format differences vs upstream V1

- Authentication headers use the `PRED_*` prefix (upstream V1 uses `POLY_*`).
- HMAC secret uses **standard** base64 (upstream V1 uses URL-safe).
- `ClobAuth` is a **5-field** struct with `bytes32 scopeId` inserted between `nonce` and `message`.
- `Order` is a **13-field** struct with `bytes32 scopeId` appended at the end.
- The `ClobAuth` EIP-712 domain is **short-form** — no `verifyingContract`.
- The `Order` EIP-712 domain name is `"Prediction Market Protocol"`, **not** the upstream V1 exchange domain.

Full diff table: [`docs/diff-vs-upstream-v1.md`](docs/diff-vs-upstream-v1.md).

### 5. Private-key handling

- The EOA private key is **never** read from an environment variable — there is no `PM_PRIVATE_KEY`. A secret in the environment leaks via `/proc/<pid>/environ` and to child processes.
- The key comes from the `--private-key` flag or, preferably, the `config.toml` file (written by `predict-cli wallet create` / `wallet import` / `setup`, mode 0600).
- Non-secret configuration (network, tenant, chain id, scope id, output, config dir) may still use `PM_*` env vars; only the private key is restricted.

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
