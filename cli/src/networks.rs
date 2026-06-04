//! Built-in network registry.
//!
//! Each supported network's full definition (chain id, RPC, tenant domain + endpoints, and all
//! contract addresses) is a YAML document baked into the binary via `include_str!` and decoded
//! through [`crate::network_config`] — the same schema and decoder the on-chain
//! `approve` / `ctf` paths already use, so a selected network resolves to exactly the bytes those
//! paths used to load from `--network-config <yaml>`.
//!
//! Selection precedence (see [`effective_network_name`]): `--network` flag / `PM_NETWORK` env >
//! `config.toml` `network` field > [`DEFAULT_NETWORK`].
//!
//! Note: the registry lives in the CLI, not the SDK. The SDK (`predict-rs-clob-client`) stays
//! address- and chain-agnostic — every contract address / endpoint / chain id is still passed in
//! by the caller. The CLI is simply a caller that ships a built-in set.

use anyhow::{Result, anyhow};

use crate::cli::Cli;
use crate::config_store::StoredConfig;
use crate::network_config::{self, NetworkConfig};

/// Network selected when neither `--network` nor `config.toml` specifies one.
pub const DEFAULT_NETWORK: &str = "monad";

const MONAD_YAML: &str = include_str!("networks/monad.yaml");

/// Names of every built-in network, for help text and error messages.
pub fn names() -> &'static [&'static str] {
    &["monad"]
}

/// Decode the built-in [`NetworkConfig`] for `name`. Errors if `name` is not a known network.
pub fn get(name: &str) -> Result<NetworkConfig> {
    let raw = match name {
        "monad" => MONAD_YAML,
        other => {
            return Err(anyhow!(
                "unknown network '{other}' (known: {})",
                names().join(", ")
            ));
        }
    };
    network_config::parse(raw)
        .map_err(|e| anyhow!("built-in network '{name}' failed to parse: {e}"))
}

/// Resolve the active network name. Precedence: `--network` flag / env > `config.toml` > default.
pub fn effective_network_name(args: &Cli, stored: Option<&StoredConfig>) -> String {
    args.network
        .clone()
        .or_else(|| stored.and_then(|c| c.network.clone()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NETWORK.to_owned())
}

/// Load the [`StoredConfig`] (for the `network` override) and return the active [`NetworkConfig`].
pub fn effective_network(args: &Cli) -> Result<NetworkConfig> {
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    get(&effective_network_name(args, stored.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_network_is_monad() {
        assert_eq!(DEFAULT_NETWORK, "monad");
    }

    #[test]
    fn monad_builtin_parses_with_expected_anchors() {
        let cfg = get("monad").expect("monad network parses");
        assert_eq!(cfg.network.name, "monad");
        assert_eq!(cfg.network.chain_id, 143);
        assert_eq!(cfg.tenant.domain, "hermestrade.xyz");
        // The exchange the order signer binds to — a wrong value here would only surface as a
        // live order rejected for a bad EIP-712 verifyingContract, so pin it in a test.
        assert_eq!(
            cfg.contracts.ctf_exchange,
            "0x017641abFa4264121237023f9Fe678BF00F60De8"
        );
        assert_eq!(
            cfg.contracts.collateral(),
            Some("0xb7bD080Df56FA76ce6CA4fA737d47815f7F8e746")
        );
        assert!(cfg.tenant.endpoints.relayer.is_some());
    }

    #[test]
    fn unknown_network_errors() {
        assert!(get("polygon").is_err());
    }
}
