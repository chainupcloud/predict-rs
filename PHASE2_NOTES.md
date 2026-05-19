# Phase 2.1 — integration handoff

Branch: `feat/phase2-auth`. Three commits, all tests passing per commit (Phase 1 golden vectors untouched, no breaking SDK API changes).

> **Status:** superseded by Phase 2.2 (`feat/phase2-orders`) — see
> [`PHASE2_ORDERS_NOTES.md`](./PHASE2_ORDERS_NOTES.md). The L1/L2 + balance-allowance
> surface documented below is unchanged on `dev`; Phase 2.2 adds the order / trade
> endpoints on top.

## Endpoints implemented

| Method | Path | Auth | Client method | CLI subcommand |
|--------|------|------|---------------|----------------|
| POST | `/auth/api-key` | L1 | `Client::create_api_key` | `pm auth create-key` |
| GET | `/auth/derive-api-key` | L1 | `Client::derive_api_key` | `pm auth derive-key` |
| DELETE | `/auth/api-key` | L1 | `Client::delete_api_key` | `pm auth delete-key` |
| — | (idempotent wrapper) | L1 | `Client::create_or_derive_api_key` | (used internally by every L2 CLI subcommand) |
| GET | `/auth/api-keys` | L2 | `Client::api_keys` | `pm auth list-keys` |
| GET | `/balance-allowance` | L2 | `Client::balance_allowance` | `pm balance --asset-type ...` |
| GET | `/balance-allowance/update` | L2 | `Client::update_balance_allowance` | `pm balance --update --asset-type ...` |

## New public types (re-exported at the crate root)

- `pm_rs_clob_client::Credentials` (already existed in Phase 1; now actually used).
- `pm_rs_clob_client::PMCup26Signer` (re-export of the existing signer struct for ergonomic L1 calls).
- `pm_rs_clob_client::AssetType` — enum `{ Collateral, Conditional }`, serialises to `"COLLATERAL"` / `"CONDITIONAL"` matching the server's `c.Query("asset_type")` check.
- `pm_rs_clob_client::ApiKeyInfo` — response shape for `/auth/api-keys`, includes the chainup-specific `proxy_wallet` field.
- `pm_rs_clob_client::BalanceAllowanceResponse` — response shape for `/balance-allowance`, includes the optional `virtual_available` / `locked` fields the server adds when the virtual-balance manager is enabled.

## New public helpers in `pm_rs_clob_client::auth`

- `auth::build_l1_headers(signer, nonce)` — uses wall-clock time.
- `auth::build_l1_headers_with_timestamp(signer, ts, nonce)` — deterministic / test-friendly variant.
- `auth::build_l2_headers(creds, address, timestamp, method, path, body)` — five-header `HeaderMap`.
- `auth::compute_l2_hmac(secret, timestamp, method, path, body)` — primitive (pre-existing; unchanged).
- `auth::sign_l2(creds, ...)` — credentials-wrapped variant of the above.
- `auth::current_timestamp()` — Unix-seconds string.
- `auth::header::{PRED_*}` — the five (six with `PRED_SCOPE_ID`) header-name constants.

## New CLI flags

| Flag | Env var | Purpose |
|------|---------|---------|
| `--private-key` | `PM_PRIVATE_KEY` (hidden in help) | EOA private key for L1 EIP-712 signing. Required by every Phase 2 subcommand. |
| `--exchange-address` | `PM_EXCHANGE_ADDRESS` | CTFExchange address. Accepted up-front for Phase 2.2 order placement; currently unused by 2.1 paths. |
| `--credentials` | `PM_CREDENTIALS_FILE` | Pre-stored L2 credentials JSON. When absent, L2 commands auto-derive via `create_or_derive_api_key`. |

## What's new on `ClientBuilder`

- `ClientBuilder::signer_address(Address)` — must be set when using L2 auth. The SDK refuses L2 calls (`Error::Validation`) when the signer address is missing.
- `ClientBuilder::credentials(Credentials)` — unchanged, but now actually consumed.

## Verification

`cargo test --workspace` is green (29 test cases):

- `clob-client` unit tests: 14 (signer, types, endpoints, **L1 header bytes**, **L2 HMAC cross-check**).
- `clob-client` integration tests (`tests/golden_signer.rs`): 3 — Phase 1 byte-for-byte signer vectors, **not modified**.
- `clob-client` integration tests (`tests/auth_flow.rs`, new): 12 — full round-trip through `wiremock` covering:
  - L1 header set for create / derive / delete.
  - `create_or_derive_api_key` 409 → derive fallback, and 200 short-circuit.
  - L2 header set for `api_keys`, `balance_allowance`, `update_balance_allowance`.
  - The critical assertion that **the L2 signature is computed over the path only**, NOT path+query.
  - `Error::Validation` short-circuit for the `asset_type` × `token_id` matrix.
  - `Error::NotAuthenticated` short-circuit when credentials are absent.
- 1 doctest (Phase 1 — unchanged).

`cargo build --release` succeeds.

## Wire-level decisions and the evidence behind them

| Decision | Evidence |
|----------|----------|
| HMAC over **path only**, NOT path+query | `services/clob-service/internal/tradingapi/middleware/auth.go::L2AuthMiddleware` line 122 calls `computeHMAC(..., c.Request.URL.Path, body)`. `URL.Path` excludes the query. The `auth_flow.rs::balance_allowance_signs_path_only_not_query` test reproduces this and asserts the on-wire signature matches the path-only computation. |
| Standard base64 (not URL-safe) | Both `middleware/auth.go::computeHMAC` and `crypto/credentials.go::GenerateSecret` use `base64.StdEncoding`. The Rust SDK uses `base64::engine::general_purpose::STANDARD`. Confirmed by `auth::tests::hmac_matches_message_layout`, which re-encodes the rs-clob-client URL-safe reference vector to standard alphabet and asserts equality. |
| `PRED_SCOPE_ID` omitted when scope is zero | `handlers/auth.go::validateL1Headers` accepts `scopeHex == ""` as "no scope binding". `auth::build_l1_headers_with_timestamp` skips the header when `signer.scope_id().is_zero()`. Tested by `auth::tests::build_l1_headers_omits_scope_when_zero`. |
| `v ∈ {0, 1}` for L1 signatures | The signer's `signature_to_bytes` writes `u8::from(sig.v())` which is `{0, 1}`. The server normalises `v ≥ 27` for compatibility (`eip712.go:86`). |
| Signature hex format = `0x` + 130 chars | `handlers/auth.go` calls `stripHexPrefix(signature)` and expects 65 raw bytes. The SDK produces `0x` + `hex::encode(65 bytes)`. |
| Body is `""` for GET / DELETE / and the POST `/auth/api-key` create call | Server-side L1 handlers never read a request body. The SDK's `create_api_key` sends no body. Confirmed by the `auth_flow.rs` mocked-server interactions — the mocked POST receives an empty body. |

## Known limitations / handoff questions

1. **`Client::delete_api_key` hard-codes `nonce = 0`.** The CLI `pm auth delete-key --nonce N` for `N != 0` errors up front with a clear message. The SDK helper signature accepts a `Uuid` for symmetry with rs-clob-client, but the server identifies the row by `(address, scope, nonce)`, so the UUID is a no-op. Adding `delete_api_key_with_nonce(signer, nonce)` is a 5-line addition; deferred to Phase 2.2 because no current consumer needs non-zero nonces.
2. **No live smoke-test of `DELETE /auth/api-key`.** The CI / dev clob-api endpoint is shared and revoking a real key would disrupt other developers. The wiremock-backed integration test verifies the wire contract; pre-merge, the orchestrator should do a one-off `pm auth create-key` → `pm auth list-keys` → `pm auth delete-key` against a throwaway scope on the dev environment.
3. **`AssetType` validation is duplicated on client + server.** The SDK rejects `Collateral + token_id` and `Conditional` without token up front (matching `handlers.go:1505-1517`). Saves a round-trip; if the server contract ever relaxes, the SDK rule is the bottleneck.
4. **No client-side Safe-address derivation.** Phase 2.1 lets the server do the derivation. If callers want to verify the Safe address before depositing, Phase 2.2 should add `signer::derive_safe_address(eoa, scope_id, factory_contract_addr, master_copy, proxy_creation_code)` — see the analysis in `clob-service/CLAUDE.md` "Safe 钱包架构" section.
5. **`create_or_derive_api_key` fallback runs on any `Error::Api` (any HTTP status error).** Polymarket V1 narrows the fallback to `ErrorKind::Status`. Our chainup `Error` is flatter (`Api { status, ... }`) so we accept that any 4xx triggers fallback. The server's behaviour on duplicate-create is "return existing creds with 200" (see `handlers/auth.go:139`), so in practice the fallback only runs on truly unexpected codes. Not a problem in any scenario we can identify; flagging for awareness.

## Files changed / added (top-level)

```
clob-client/Cargo.toml                    + wiremock dev-dep
clob-client/src/auth.rs                   ≈+200 lines: L1/L2 header builders, header-name constants, tests
clob-client/src/client.rs                 ≈+170 lines: 7 new methods, request_authenticated plumbing, signer_address
clob-client/src/clob/types.rs             ≈+70  lines: AssetType, ApiKeyInfo, BalanceAllowanceResponse
clob-client/src/lib.rs                    re-exports
clob-client/tests/auth_flow.rs            new file, 12 integration tests using wiremock
cli/Cargo.toml                            + secrecy, uuid
cli/src/cli.rs                            + AuthCommand, BalanceArgs, SignatureTypeArg, AssetTypeArg
cli/src/commands.rs                       + run_auth, run_balance, with_l2_credentials helper
docs/auth-flow.md                         new file, sequence diagrams + HMAC formula
docs/diff-vs-polymarket-v1.md             minor updates for Phase 2.1
PHASE2_NOTES.md                           this file
```
