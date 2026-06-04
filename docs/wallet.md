# `predict-cli wallet` — local key + config-file management

`predict-cli wallet` persists a single secp256k1 private key plus a few signing-related defaults
to disk so that every other `predict-cli` subcommand can sign without the caller passing
`--private-key` on every invocation.

The store is a TOML file at `<config-dir>/config.toml`. The directory is `--config-dir <path>`
when given, otherwise `dirs::config_dir()/predict` — on Linux `~/.config/predict`, on macOS
`~/Library/Application Support/predict`.

The file is created with mode `0600` (Unix); its parent directory is `0700`. Writes go
through a sibling temp file and `rename(2)` so a crash mid-write cannot truncate the
existing config.

## Subcommands

### `predict-cli wallet create [--force]`

Generates a fresh secp256k1 private key (`PrivateKeySigner::random`) and writes it to
`config.toml`. Refuses to overwrite an existing entry unless `--force` is set.

```
$ predict-cli wallet create
Generated new wallet
address: 0x9a72e5...c7f1
saved  : /home/me/.config/predict/config.toml
```

### `predict-cli wallet import <0xHEX> [--force]`

Accepts a 32-byte hex private key (with or without the `0x` prefix), validates it, and
persists it. Same overwrite policy as `create`.

```
$ predict-cli wallet import 0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318
Imported wallet
address: 0x9a72e5...c7f1
saved  : /home/me/.config/predict/config.toml
```

### `predict-cli wallet address`

Resolves the active key (`--private-key` flag, else `config.toml`) and prints the
derived EOA address. Useful for piping into other tools:

```
$ predict-cli wallet address
0x9a72e5...c7f1
```

### `predict-cli wallet show`

Prints the address plus where it was loaded from. Reports `cli (--private-key)`
when the key came from the global flag, or
`config-file <path>` when it was loaded from disk. Reports `none` if nothing is configured.

```
$ predict-cli wallet show
address    : 0x9a72e5...c7f1
source     : config-file /home/me/.config/predict/config.toml
config path: /home/me/.config/predict/config.toml
```

The raw private key is never printed by any subcommand.

### `predict-cli wallet reset [--force]`

Deletes `config.toml`. Prompts `y/N` unless `--force` is set.

```
$ predict-cli wallet reset
Delete /home/me/.config/predict/config.toml and forget the stored wallet? [y/N] y
removed /home/me/.config/predict/config.toml
```

## Resolution order for other commands

The config file is also consulted by every command that needs a key, chain id, scope id,
network, or Safe maker. For each value the matching flag wins; otherwise `config.toml` is used;
otherwise it falls back to the selected network (default `monad`).

| Value        | Flag            | Fallback                               |
|--------------|-----------------|----------------------------------------|
| private key  | `--private-key` | `config.toml`                          |
| chain id     | `--chain-id`    | `config.toml`, else the `monad` network (143) |
| scope id     | `--scope-id`    | `config.toml`                          |
| network      | `--network`     | `config.toml`, else `monad`            |
| Safe maker   | `--maker`       | `config.toml` `safe_address`           |

`predict-cli setup` writes the same `config.toml`; see `predict-cli setup --help` for the guided
wizard (wallet / scopeId / Safe / L2 key).

## On-disk schema

A complete Monad / hermestrade.xyz config (a copy-ready template ships at
[`examples/config.toml`](../examples/config.toml)):

```toml
private_key    = "0xYOUR_64_HEX_PRIVATE_KEY"   # your EOA; mode 0600, never read from env
safe_address   = "0xYOUR_SAFE_ADDRESS"         # gnosis-safe: the Safe holds funds and is the maker
network        = "monad"                        # supplies chain id 143, endpoints, exchange, contracts
scope_id       = "0x1811a132dd725e2c40475aa52df39025b36544f7a70825968e32b28da2196e95"
signature_type = "gnosis-safe"
# Optional — already provided by the monad network:
# tenant       = "hermestrade.xyz"
# chain_id     = 143
```

Only `private_key` (plus `safe_address` in gnosis-safe mode) must be filled in; every other
field defaults from the selected `network`. The private key is never read from an environment
variable — it lives here at mode 0600, or comes from `--private-key`.

## Tests

The store is exercised by unit tests in `cli/src/config_store.rs` and
`cli/src/wallet_commands.rs`. Tests redirect the config directory to a `tempfile::TempDir`
so they never touch the real `~/.config/predict`.
