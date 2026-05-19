//! `pm approve` subcommands.
//!
//! **This file ships `check` only — `set` (broadcasting on-chain approvals) is intentionally
//! held back until the Safe-vs-EOA approval flow on chainup is confirmed end-to-end.** See
//! `pm-rs/.local/TODO.md` for the open question.
//!
//! `check` reads, per the configured tenant network YAML:
//!   - `IERC20.allowance(owner, spender)` for the USDC contract against each approval target.
//!   - `IERC1155.isApprovedForAll(owner, operator)` for the CTF contract against each target.
//!
//! `owner` defaults to the EOA derived from the configured wallet, but **chainup users by
//! default trade through a Safe** (signatureType=2). The Safe is the address holding USDC
//! and CTF balances — its on-chain approvals are what the server actually verifies. Pass
//! `--address <safe>` explicitly when checking a Safe owner; an SDK helper to derive the
//! Safe address from EOA + scopeId + factory is still on the P1 backlog.

use std::str::FromStr;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};

use crate::cli::Cli;
use crate::network_config::{self, ApprovalTarget, NetworkConfig};
use crate::output::{self, Format};

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IERC1155 {
        function isApprovedForAll(address account, address operator) external view returns (bool);
    }
}

#[derive(Debug, Subcommand)]
pub enum ApproveCommand {
    /// Read `USDC.allowance(owner, target)` and `CTF.isApprovedForAll(owner, target)` for
    /// each tenant approval target. No on-chain writes.
    Check(CheckArgs),
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Path to the tenant network YAML. Schema matches `examples/networks/*.yaml`.
    #[arg(long)]
    pub network_config: String,
    /// Owner address to check. Defaults to the EOA from the configured wallet.
    /// **For Safe-wallet users (`signatureType=2`, chainup default) you must pass the Safe
    /// address explicitly here** — the EOA holds no funds and its allowance is always zero.
    #[arg(long)]
    pub address: Option<String>,
    /// Override the RPC URL from the network config (e.g. for a fork node).
    #[arg(long)]
    pub rpc_url: Option<String>,
}

pub async fn run(args: &Cli, sub: &ApproveCommand, fmt: Format) -> Result<()> {
    match sub {
        ApproveCommand::Check(a) => run_check(args, a, fmt).await,
    }
}

async fn run_check(args: &Cli, a: &CheckArgs, fmt: Format) -> Result<()> {
    let cfg = network_config::load(&a.network_config)?;
    let owner = resolve_owner(args, a.address.as_deref())?;
    let rpc_url = a.rpc_url.clone().unwrap_or_else(|| cfg.network.rpc_url.clone());

    let collateral = cfg.contracts.collateral().ok_or_else(|| {
        anyhow!(
            "network config {} declares no collateral token (set `contracts.usdc:` or `contracts.wrapped_collateral:`)",
            a.network_config
        )
    })?;
    let collateral_addr = parse_addr(collateral)
        .with_context(|| format!("invalid collateral address '{collateral}'"))?;
    // Chainup uses the same CTF Exchange contract as the conditional-tokens registry holder
    // for `isApprovedForAll` lookups; the Go server queries `IERC1155.isApprovedForAll`
    // against the CTF Exchange's underlying token. For now we ask the caller's wallet which
    // ERC-1155 contract to query via the CTF Exchange — adjust once chainup ships an
    // explicit `conditional_tokens` field in the YAML.
    let ctf_addr = parse_addr(&cfg.contracts.ctf_exchange).with_context(|| {
        format!(
            "invalid ctf_exchange address '{}'",
            cfg.contracts.ctf_exchange
        )
    })?;

    let statuses = read_approvals(
        &rpc_url,
        owner,
        collateral_addr,
        ctf_addr,
        &cfg.contracts.approval_targets(),
    )
    .await?;

    print_statuses(&cfg, owner, &statuses, fmt)
}

#[derive(Debug, Clone)]
pub struct ApprovalStatus {
    pub target_name: String,
    pub target_address: String,
    pub usdc_allowance: U256,
    pub ctf_approved: bool,
    pub usdc_error: Option<String>,
    pub ctf_error: Option<String>,
}

async fn read_approvals(
    rpc_url: &str,
    owner: Address,
    collateral: Address,
    ctf: Address,
    targets: &[ApprovalTarget],
) -> Result<Vec<ApprovalStatus>> {
    let url = url::Url::parse(rpc_url).with_context(|| format!("parse rpc_url '{rpc_url}'"))?;
    let provider = ProviderBuilder::new().connect_http(url);

    let usdc = IERC20::new(collateral, provider.clone());
    let cts = IERC1155::new(ctf, provider.clone());

    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        let target_addr = parse_addr(&t.address)
            .with_context(|| format!("invalid target address '{}' for {}", t.address, t.name))?;

        let (allowance, usdc_err) = match usdc.allowance(owner, target_addr).call().await {
            Ok(v) => (v, None),
            Err(e) => (U256::ZERO, Some(e.to_string())),
        };
        let (approved, ctf_err) = match cts.isApprovedForAll(owner, target_addr).call().await {
            Ok(v) => (v, None),
            Err(e) => (false, Some(e.to_string())),
        };
        out.push(ApprovalStatus {
            target_name: t.name.to_string(),
            target_address: t.address.clone(),
            usdc_allowance: allowance,
            ctf_approved: approved,
            usdc_error: usdc_err,
            ctf_error: ctf_err,
        });
    }
    Ok(out)
}

fn resolve_owner(args: &Cli, override_address: Option<&str>) -> Result<Address> {
    if let Some(s) = override_address {
        return parse_addr(s).with_context(|| format!("invalid --address '{s}'"));
    }
    let sig_type = crate::commands::effective_signature_type(args)?;
    if sig_type == pm_rs_clob_client::types::SignatureType::Eoa {
        let (pk, _source) = crate::wallet_commands::resolve_private_key(args)?;
        let signer = parse_signer(&pk)?;
        return Ok(signer.address());
    }
    // Safe / Proxy modes: the EOA holds no funds. Use the stored Safe address.
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let safe = stored
        .as_ref()
        .and_then(|c| c.safe_address.as_deref())
        .ok_or_else(|| {
            anyhow!(
                "owner unresolved: signature_type={sig_type:?} needs a Safe address. Either:\n\
                 - run `pm wallet detect-safe` (fetches it from the chainup server, requires L2 creds), or\n\
                 - run `pm wallet set-safe <addr>` (paste it yourself), or\n\
                 - pass `--address <addr>` explicitly, or\n\
                 - re-run with `--signature-type eoa` to check the EOA instead."
            )
        })?;
    parse_addr(safe).with_context(|| format!("invalid stored safe_address '{safe}'"))
}

fn parse_signer(hex_str: &str) -> Result<alloy::signers::local::PrivateKeySigner> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(stripped).context("decode private-key hex")?;
    if bytes.len() != 32 {
        return Err(anyhow!("private key must be 32 bytes, got {}", bytes.len()));
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&bytes);
    alloy::signers::local::PrivateKeySigner::from_bytes(&buf.into())
        .map_err(|e| anyhow!("invalid private key: {e}"))
}

fn parse_addr(s: &str) -> Result<Address> {
    Address::from_str(s).map_err(|e| anyhow!("parse address '{s}': {e}"))
}

fn print_statuses(
    cfg: &NetworkConfig,
    owner: Address,
    statuses: &[ApprovalStatus],
    fmt: Format,
) -> Result<()> {
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "tenant": cfg.tenant.name,
            "network": cfg.network.name,
            "chain_id": cfg.network.chain_id,
            "owner": format!("{owner:?}"),
            "statuses": statuses.iter().map(|s| serde_json::json!({
                "target_name": s.target_name,
                "target_address": s.target_address,
                "usdc_allowance": s.usdc_allowance.to_string(),
                "ctf_approved": s.ctf_approved,
                "usdc_error": s.usdc_error,
                "ctf_error": s.ctf_error,
            })).collect::<Vec<_>>(),
        })),
        Format::Table => {
            println!("tenant : {}", cfg.tenant.name);
            println!("chain  : {} (id {})", cfg.network.name, cfg.network.chain_id);
            println!("owner  : {owner:?}");
            for s in statuses {
                println!();
                println!("{}", s.target_name);
                println!("  address     : {}", s.target_address);
                let allowance_label = if s.usdc_allowance == U256::ZERO {
                    "0".to_string()
                } else if s.usdc_allowance == U256::MAX {
                    "MAX".to_string()
                } else {
                    s.usdc_allowance.to_string()
                };
                if let Some(err) = &s.usdc_error {
                    println!("  USDC allow. : ERROR ({err})");
                } else {
                    println!("  USDC allow. : {allowance_label}");
                }
                if let Some(err) = &s.ctf_error {
                    println!("  CTF approval: ERROR ({err})");
                } else {
                    println!("  CTF approval: {}", s.ctf_approved);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_addr_accepts_checksum_form() {
        let a =
            parse_addr("0x017641abFa4264121237023f9Fe678BF00F60De8").expect("checksum address");
        assert_eq!(
            format!("{a:?}").to_lowercase(),
            "0x017641abfa4264121237023f9fe678bf00f60de8"
        );
    }

    #[test]
    fn parse_addr_rejects_garbage() {
        assert!(parse_addr("nope").is_err());
        assert!(parse_addr("0x1234").is_err());
    }
}
