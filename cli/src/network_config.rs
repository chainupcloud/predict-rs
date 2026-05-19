//! Tenant network configuration loaded from YAML.
//!
//! Mirrors the schema of `examples/networks/monad-hermestrade.yaml`. Used by `pm approve`
//! (and later `pm setup`) to find the chain id, RPC endpoint, USDC / CTF / Exchange / Safe
//! contract addresses, and the spender / operator targets a user wallet must authorise to
//! trade.
//!
//! Per `pm-rs/CLAUDE.md`: no contract address may live in SDK source. This loader keeps
//! tenant-specific addresses caller-supplied — the CLI loads `--network-config <path>`,
//! the SDK reads only what the caller passes in.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NetworkConfig {
    pub network: NetworkSection,
    pub tenant: TenantSection,
    pub contracts: ContractsSection,
    #[serde(default)]
    pub security: Option<SecuritySection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NetworkSection {
    pub name: String,
    pub chain_id: u64,
    #[serde(default)]
    pub gas_token: Option<String>,
    pub rpc_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TenantSection {
    pub name: String,
    pub domain: String,
    pub endpoints: TenantEndpoints,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TenantEndpoints {
    pub clob: String,
    #[serde(default)]
    pub gamma: Option<String>,
    #[serde(default)]
    pub ws: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub relayer: Option<String>,
}

/// All tenant-supplied contract addresses. Names match the YAML file 1:1.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ContractsSection {
    pub ctf_exchange: String,
    /// ERC-1155 ConditionalTokens contract for `isApprovedForAll(owner, operator)` lookups.
    /// On chainup Monad this is the same address as `ctf_exchange`; on other deployments it
    /// may differ.
    #[serde(default)]
    pub conditional_tokens: Option<String>,
    #[serde(default)]
    pub neg_risk_ctf_exchange: Option<String>,
    #[serde(default)]
    pub neg_risk_adapter: Option<String>,
    #[serde(default)]
    pub fee_module: Option<String>,
    #[serde(default)]
    pub neg_risk_fee_module: Option<String>,
    #[serde(default)]
    pub safe_proxy_factory: Option<String>,
    #[serde(default)]
    pub uma_ctf_adapter: Option<String>,
    #[serde(default)]
    pub neg_risk_uma_ctf_adapter: Option<String>,
    #[serde(default)]
    pub light_oracle: Option<String>,
    #[serde(default)]
    pub neg_risk_operator: Option<String>,
    #[serde(default)]
    pub uma_sports_oracle: Option<String>,
    #[serde(default)]
    pub wrapped_collateral: Option<String>,
    #[serde(default)]
    pub vault: Option<String>,
    #[serde(default)]
    pub fee_vault: Option<String>,
    #[serde(default)]
    pub usdw: Option<String>,
    #[serde(default)]
    pub usd_wrapper: Option<String>,
    /// USDC (collateral) address. Some tenant YAMLs use `wrapped_collateral` / `usdw` as the
    /// effective USDC — callers should resolve via [`Self::collateral`].
    #[serde(default)]
    pub usdc: Option<String>,
}

impl ContractsSection {
    /// Return the address that callers should treat as the collateral token. Prefers
    /// `usdc` if set, else `usdw` (chainup's wrapped USDC variant), else
    /// `wrapped_collateral` (a NegRisk-context contract that is *not* the same as USDW on
    /// some deployments). Returns `None` if none are set.
    pub fn collateral(&self) -> Option<&str> {
        self.usdc
            .as_deref()
            .or(self.usdw.as_deref())
            .or(self.wrapped_collateral.as_deref())
    }

/// The set of (name, address) pairs that a user wallet must authorise as USDC spenders
    /// and CTF operators in order to trade on this tenant. Mirrors polymarket-cli's
    /// `approval_targets()` but reads addresses from the tenant YAML rather than a hard-coded
    /// `phf_map!`.
    pub fn approval_targets(&self) -> Vec<ApprovalTarget> {
        let mut out = vec![ApprovalTarget {
            name: "CTF Exchange",
            address: self.ctf_exchange.clone(),
        }];
        if let Some(a) = &self.neg_risk_ctf_exchange {
            out.push(ApprovalTarget {
                name: "Neg Risk CTF Exchange",
                address: a.clone(),
            });
        }
        if let Some(a) = &self.neg_risk_adapter {
            out.push(ApprovalTarget {
                name: "Neg Risk Adapter",
                address: a.clone(),
            });
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SecuritySection {
    #[serde(default)]
    pub contract_whitelist: Vec<String>,
    #[serde(default)]
    pub multi_send_address: Option<String>,
    #[serde(default)]
    pub safe_proxy_factory: Option<String>,
}

/// One target address that the user must approve as a spender (USDC) and operator (CTF).
#[derive(Debug, Clone)]
pub struct ApprovalTarget {
    pub name: &'static str,
    pub address: String,
}

pub fn load(path: impl AsRef<Path>) -> Result<NetworkConfig> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read network config {}", path.display()))?;
    serde_yaml::from_str::<NetworkConfig>(&raw)
        .with_context(|| format!("decode network config {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_monad_hermestrade_example() {
        let cfg = load("../examples/networks/monad-hermestrade.yaml").expect("load yaml");
        assert_eq!(cfg.network.name, "monad");
        assert_eq!(cfg.network.chain_id, 143);
        assert_eq!(cfg.network.rpc_url, "https://rpc.monad.xyz");
        assert_eq!(cfg.tenant.name, "hermestrade");
        assert_eq!(cfg.tenant.domain, "hermestrade.xyz");
        assert_eq!(
            cfg.tenant.endpoints.clob,
            "https://clob-api.hermestrade.xyz"
        );

        // The Monad example uses `wrapped_collateral` (USDW) — not a literal `usdc:` key.
        // The collateral helper falls back to that field.
        assert!(cfg.contracts.collateral().is_some());

        let targets = cfg.contracts.approval_targets();
        assert_eq!(targets.len(), 3);
        assert_eq!(targets[0].name, "CTF Exchange");
        assert_eq!(targets[1].name, "Neg Risk CTF Exchange");
        assert_eq!(targets[2].name, "Neg Risk Adapter");
    }

    #[test]
    fn collateral_prefers_usdc_then_usdw_then_wrapped() {
        let with_usdc = ContractsSection {
            ctf_exchange: "0x1".into(),
            usdc: Some("0xUSDC".into()),
            wrapped_collateral: Some("0xWC".into()),
            usdw: Some("0xUSDW".into()),
            ..ContractsSection::empty_for_test()
        };
        assert_eq!(with_usdc.collateral(), Some("0xUSDC"));

        // No usdc but usdw present → usdw wins over wrapped_collateral.
        let no_usdc = ContractsSection {
            ctf_exchange: "0x1".into(),
            usdc: None,
            wrapped_collateral: Some("0xWC".into()),
            usdw: Some("0xUSDW".into()),
            ..ContractsSection::empty_for_test()
        };
        assert_eq!(no_usdc.collateral(), Some("0xUSDW"));

        // Only wrapped_collateral set → fallback to it.
        let only_wc = ContractsSection {
            ctf_exchange: "0x1".into(),
            usdc: None,
            usdw: None,
            wrapped_collateral: Some("0xWC".into()),
            ..ContractsSection::empty_for_test()
        };
        assert_eq!(only_wc.collateral(), Some("0xWC"));
    }

    impl ContractsSection {
        fn empty_for_test() -> Self {
            Self {
                ctf_exchange: String::new(),
                conditional_tokens: None,
                neg_risk_ctf_exchange: None,
                neg_risk_adapter: None,
                fee_module: None,
                neg_risk_fee_module: None,
                safe_proxy_factory: None,
                uma_ctf_adapter: None,
                neg_risk_uma_ctf_adapter: None,
                light_oracle: None,
                neg_risk_operator: None,
                uma_sports_oracle: None,
                wrapped_collateral: None,
                vault: None,
                fee_vault: None,
                usdw: None,
                usd_wrapper: None,
                usdc: None,
            }
        }
    }
}
