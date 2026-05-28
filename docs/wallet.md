# `pm wallet` — local key + config-file management

`pm wallet` persists a single secp256k1 private key plus a few signing-related defaults
to disk so that every other `pm` subcommand can sign without the caller passing
`--private-key` on every invocation.

The store is a TOML file at `<config-dir>/config.toml`. The directory is resolved in this
order, first match wins:

1. `--config-dir <path>` (global flag)
2. `PM_CONFIG_DIR` environment variable
3. `dirs::config_dir()/pm` — on Linux that is `$XDG_CONFIG_HOME/pm` or `~/.config/pm`;
   on macOS `~/Library/Application Support/pm`.

The file is created with mode `0600` (Unix); its parent directory is `0700`. Writes go
through a sibling temp file and `rename(2)` so a crash mid-write cannot truncate the
existing config.

## Subcommands

### `pm wallet create [--force]`

Generates a fresh secp256k1 private key (`PrivateKeySigner::random`) and writes it to
`config.toml`. Refuses to overwrite an existing entry unless `--force` is set.

```
$ pm wallet create
Generated new wallet
address: 0x9a72e5...c7f1
saved  : /home/me/.config/pm/config.toml
```

### `pm wallet import <0xHEX> [--force]`

Accepts a 32-byte hex private key (with or without the `0x` prefix), validates it, and
persists it. Same overwrite policy as `create`.

```
$ pm wallet import 0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318
Imported wallet
address: 0x9a72e5...c7f1
saved  : /home/me/.config/pm/config.toml
```

### `pm wallet address`

Resolves the active key using the full chain (flag → env → config file) and prints the
derived EOA address. Useful for piping into other tools:

```
$ pm wallet address
0x9a72e5...c7f1
```

### `pm wallet show`

Prints the address plus where it was loaded from. Reports `cli (--private-key / PM_PRIVATE_KEY)`
when the key came from the global flag or env (clap merges both), or
`config-file <path>` when it was loaded from disk. Reports `none` if nothing is configured.

```
$ pm wallet show
address    : 0x9a72e5...c7f1
source     : config-file /home/me/.config/pm/config.toml
config path: /home/me/.config/pm/config.toml
```

The raw private key is never printed by any subcommand.

### `pm wallet reset [--force]`

Deletes `config.toml`. Prompts `y/N` unless `--force` is set.

```
$ pm wallet reset
Delete /home/me/.config/pm/config.toml and forget the stored wallet? [y/N] y
removed /home/me/.config/pm/config.toml
```

## Resolution order for other commands

The config file is also consulted as a fallback by every command that needs a key:

| Field            | 1st                      | 2nd                | 3rd                   |
|------------------|--------------------------|--------------------|-----------------------|
| private key      | `--private-key`          | `PM_PRIVATE_KEY`   | `config.toml`         |
| chain id         | `--chain-id`             | `PM_CHAIN_ID`      | `config.toml`         |
| scope id         | `--scope-id`             | `PM_SCOPE_ID`      | `config.toml`         |

`pm setup` writes the same `config.toml`; see `pm setup --help` for the interactive
wizard that walks through wallet / chain / scope-id selection.

## On-disk schema

```toml
private_key    = "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
chain_id       = 11155420
scope_id       = "0x0000000000000000000000000000000000000000000000000000000000000001"
signature_type = "gnosis-safe"
```

All fields are optional; any missing field falls through to the lower-priority sources.

## Tests

The store is exercised by unit tests in `cli/src/config_store.rs` and
`cli/src/wallet_commands.rs`. Tests redirect the config directory via the same
`--config-dir` / `PM_CONFIG_DIR` mechanism (using `tempfile::TempDir`) so they never
touch the real `~/.config/pm`.
