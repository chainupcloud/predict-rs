//! `predict-cli approve` subcommands.
//!
//! - `check` reads `IERC20.allowance(owner, spender)` and `IERC1155.isApprovedForAll`
//!   per the tenant YAML's approval targets. No on-chain writes.
//! - `set` writes `usdw.approve(spender, amount)` through the user's Safe, using the
//!   `relayer-service` to pay gas. Defaults to dry-run; `--execute` actually
//!   submits. Safe-mode (`signatureType=2`) only — EOA mode is not supported.
//!
//! `owner` for `check` defaults to the EOA derived from the configured wallet, but
//! **users by default trade through a Safe** (signatureType=2). The Safe is the
//! address holding USDW and CTF balances. Pass `--address <safe>` when checking a Safe
//! owner — for `set`, the Safe address comes from the local config and the EOA is the
//! Safe owner that signs the `SafeTx` EIP-712 payload.

use std::str::FromStr;
use std::time::Duration;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use predict_rs_clob_client::safe::multisend::SafeSubOp;
use predict_rs_clob_client::safe::{self, SafeTransaction};

use crate::cli::Cli;
use crate::network_config::{self, ApprovalTarget, NetworkConfig};
use crate::output::{self, Format};
use crate::safe_exec::{self, SafeContext};

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
    }

    #[sol(rpc)]
    interface IERC1155 {
        function isApprovedForAll(address account, address operator) external view returns (bool);
        function setApprovalForAll(address operator, bool approved) external;
    }
}

#[derive(Debug, Subcommand)]
pub enum ApproveCommand {
    /// Read `USDW.allowance(owner, target)` and `CTF.isApprovedForAll(owner, target)` for
    /// each tenant approval target. No on-chain writes.
    Check(CheckArgs),
    /// Safe-mode `usdw.approve(spender, amount)` via the relayer-service. Default
    /// dry-run; pass `--execute` to submit. Single op when `--spender` is provided, else
    /// batches every entry in the YAML's `approval_targets()` via MultiSend.
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Path to the tenant network YAML. Schema matches `examples/networks/*.yaml`.
    #[arg(long)]
    pub network_config: String,
    /// Owner address to check. Defaults to the EOA from the configured wallet.
    /// **For Safe-wallet users (`signatureType=2`, default) you must pass the Safe
    /// address explicitly here** — the EOA holds no funds and its allowance is always zero.
    #[arg(long)]
    pub address: Option<String>,
    /// Override the RPC URL from the network config (e.g. for a fork node).
    #[arg(long)]
    pub rpc_url: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AssetSet {
    /// Only `USDW.approve(spender, amount)` ops.
    Usdw,
    /// Only `CTF.setApprovalForAll(operator, true)` ops.
    Ctf,
    /// Both USDW.approve and CTF.setApprovalForAll for every target — what a fresh
    /// wallet needs to fully trade. Matches the upstream V1 `set` defaults.
    All,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// Path to the tenant network YAML. Schema matches `examples/networks/*.yaml`.
    #[arg(long)]
    pub network_config: String,
    /// Which approvals to issue. `all` (default) = USDW.approve(target, MAX) +
    /// CTF.setApprovalForAll(target, true) for every approval target — what a
    /// fresh wallet needs to actually place orders. `usdw` / `ctf` limit the
    /// batch to one asset.
    #[arg(long, value_enum, default_value_t = AssetSet::All)]
    pub asset: AssetSet,
    /// Single spender to approve. Default: every entry returned by `approval_targets()`
    /// (CtfExchange + NegRiskCtfExchange + NegRiskAdapter) bundled into one MultiSend.
    #[arg(long)]
    pub spender: Option<String>,
    /// USDW approve amount in raw smallest unit. Use `MAX` for `uint256::MAX`. Default
    /// `MAX`. Ignored when `--asset ctf` (CTF approval is a boolean, not an amount).
    #[arg(long, default_value = "MAX")]
    pub amount: String,
    /// Actually submit the SafeTx. Without this flag the command runs in dry-run mode —
    /// it signs locally and prints the `SubmitRequest` body but never POSTs to the relayer.
    #[arg(long)]
    pub execute: bool,
    /// Poll interval (seconds) when waiting for the relayer to confirm. Default 2 s.
    #[arg(long, default_value_t = 2)]
    pub poll_interval_secs: u64,
    /// Polling deadline (seconds). Default 60 s. After this, the command returns the most
    /// recently observed [`TransactionState`] even if not yet terminal.
    #[arg(long, default_value_t = 60)]
    pub poll_timeout_secs: u64,
    /// Override `network.rpc_url` from the YAML — used to read the Safe's current nonce.
    #[arg(long)]
    pub rpc_url: Option<String>,
    /// EIP-712 LoginMessage `domain` field. Defaults to the YAML's `tenant.domain`.
    #[arg(long)]
    pub gamma_domain: Option<String>,
    /// EIP-712 LoginMessage `uri` field. Defaults to `https://<tenant.domain>`.
    #[arg(long)]
    pub gamma_uri: Option<String>,
}

pub async fn run(args: &Cli, sub: &ApproveCommand, fmt: Format) -> Result<()> {
    match sub {
        ApproveCommand::Check(a) => run_check(args, a, fmt).await,
        ApproveCommand::Set(a) => run_set(args, a, fmt).await,
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
    // Prefer the explicit `conditional_tokens` field when set; fall back to `ctf_exchange`
    // for backward compatibility with older YAMLs (and Monad where the two are the
    // same contract).
    let ctf_source = cfg
        .contracts
        .conditional_tokens
        .as_deref()
        .unwrap_or(cfg.contracts.ctf_exchange.as_str());
    let ctf_addr = parse_addr(ctf_source)
        .with_context(|| format!("invalid conditional_tokens / ctf_exchange address '{ctf_source}'"))?;

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
    if sig_type == predict_rs_clob_client::types::SignatureType::Eoa {
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
                 - run `predict-cli wallet detect-safe` (fetches it from the server, requires L2 creds), or\n\
                 - run `predict-cli wallet set-safe <addr>` (paste it yourself), or\n\
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

// ─── predict-cli approve set ─────────────────────────────────────────────────────

/// `usdw.approve(spender, amount)` selector — keccak256("approve(address,uint256)")[..4].
const ERC20_APPROVE_SELECTOR: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];

/// Encode the calldata for `IERC20.approve(spender, amount)` by hand. We do this rather
/// than going through `alloy::sol!`'s contract bindings to keep the call free of an
/// RPC `Provider` instance — we never broadcast directly, the relayer does.
fn encode_approve(spender: Address, amount: U256) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32 + 32);
    out.extend_from_slice(&ERC20_APPROVE_SELECTOR);
    let mut spender_pad = [0u8; 32];
    spender_pad[12..].copy_from_slice(spender.as_slice());
    out.extend_from_slice(&spender_pad);
    out.extend_from_slice(&amount.to_be_bytes::<32>());
    out
}

fn parse_amount(raw: &str) -> Result<U256> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("MAX") || trimmed.eq_ignore_ascii_case("UINT256_MAX") {
        return Ok(U256::MAX);
    }
    if let Some(rest) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        return U256::from_str_radix(rest, 16)
            .map_err(|e| anyhow!("invalid hex amount '{trimmed}': {e}"));
    }
    U256::from_str_radix(trimmed, 10).map_err(|e| anyhow!("invalid amount '{trimmed}': {e}"))
}

/// Resolve the spender list. `--spender` takes precedence; otherwise use the YAML's
/// `approval_targets()`.
fn resolve_spenders(cfg: &NetworkConfig, override_spender: Option<&str>) -> Result<Vec<(String, Address)>> {
    if let Some(s) = override_spender {
        let addr = Address::from_str(s.trim())
            .map_err(|e| anyhow!("invalid --spender '{s}': {e}"))?;
        return Ok(vec![("(--spender override)".to_owned(), addr)]);
    }
    let targets: Vec<ApprovalTarget> = cfg.contracts.approval_targets();
    if targets.is_empty() {
        bail!(
            "network config {} declares no approval targets — set `contracts.ctf_exchange` (and optionally `neg_risk_ctf_exchange`, `neg_risk_adapter`)",
            cfg.tenant.name
        );
    }
    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        let addr = Address::from_str(&t.address)
            .with_context(|| format!("invalid approval-target address '{}' ({})", t.address, t.name))?;
        out.push((t.name.to_owned(), addr));
    }
    Ok(out)
}

fn require_collateral(cfg: &NetworkConfig) -> Result<(String, Address)> {
    let raw = cfg.contracts.usdw.as_deref().or(cfg.contracts.collateral()).ok_or_else(|| {
        anyhow!(
            "network config declares no USDW / collateral address (set `contracts.usdw` in the YAML)"
        )
    })?;
    let addr = Address::from_str(raw)
        .with_context(|| format!("invalid USDW address '{raw}'"))?;
    Ok((raw.to_owned(), addr))
}

fn require_multisend(cfg: &NetworkConfig) -> Result<Address> {
    let raw = cfg.security.as_ref().and_then(|s| s.multi_send_address.as_deref())
        .ok_or_else(|| anyhow!("network config declares no `security.multi_send_address` — required for batched approvals"))?;
    Address::from_str(raw).with_context(|| format!("invalid multi_send_address '{raw}'"))
}

fn require_conditional_tokens(cfg: &NetworkConfig) -> Result<Address> {
    let raw = cfg.contracts.conditional_tokens.as_deref().ok_or_else(|| {
        anyhow!(
            "network config declares no `contracts.conditional_tokens` — required for CTF approvals"
        )
    })?;
    Address::from_str(raw)
        .with_context(|| format!("invalid conditional_tokens address '{raw}'"))
}

/// `setApprovalForAll(address operator, bool approved)` selector.
const ERC1155_SET_APPROVAL_FOR_ALL_SELECTOR: [u8; 4] = [0xa2, 0x2c, 0xb4, 0x65];

fn encode_set_approval_for_all(operator: Address, approved: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32 + 32);
    out.extend_from_slice(&ERC1155_SET_APPROVAL_FOR_ALL_SELECTOR);
    let mut op_pad = [0u8; 32];
    op_pad[12..].copy_from_slice(operator.as_slice());
    out.extend_from_slice(&op_pad);
    let mut bool_pad = [0u8; 32];
    if approved {
        bool_pad[31] = 1;
    }
    out.extend_from_slice(&bool_pad);
    out
}

/// One sub-operation in the planned batch, retained so we can render it for the user.
struct PlannedOp {
    summary: String,
    detail: String,
    sub_op: SafeSubOp,
}

async fn run_set(args: &Cli, a: &SetArgs, fmt: Format) -> Result<()> {
    let cfg = network_config::load(&a.network_config)?;

    // 1. Resolve identity (signature_type guard + EOA + Safe + scope).
    let ctx = SafeContext::resolve(args, cfg, a.rpc_url.as_deref())?;

    // 2. Resolve assets + spenders + amount.
    let (usdw_raw, usdw) = require_collateral(&ctx.cfg)?;
    let amount = parse_amount(&a.amount)?;
    let spenders = resolve_spenders(&ctx.cfg, a.spender.as_deref())?;
    if spenders.is_empty() {
        bail!("no spenders to approve");
    }

    // 3. Build the planned op list. Order: every USDW.approve op, then every
    //    CTF.setApprovalForAll op. Order doesn't matter on-chain (each is idempotent and
    //    sets independent state), but a stable order keeps the table output predictable.
    let mut planned: Vec<PlannedOp> = Vec::with_capacity(spenders.len() * 2);
    let amount_label = if amount == U256::MAX { "MAX".to_owned() } else { amount.to_string() };

    let include_usdw = matches!(a.asset, AssetSet::Usdw | AssetSet::All);
    let include_ctf = matches!(a.asset, AssetSet::Ctf | AssetSet::All);

    if include_usdw {
        for (name, sp) in &spenders {
            planned.push(PlannedOp {
                summary: format!("USDW.approve → {name} ({sp:#x})"),
                detail: format!("approve({usdw_raw}, {amount_label})"),
                sub_op: SafeSubOp::call(usdw, encode_approve(*sp, amount)),
            });
        }
    }

    if include_ctf {
        let ctf_addr = require_conditional_tokens(&ctx.cfg)?;
        for (name, op) in &spenders {
            planned.push(PlannedOp {
                summary: format!("CTF.setApprovalForAll → {name} ({op:#x})"),
                detail: format!("setApprovalForAll(operator={op:#x}, true) on {ctf_addr:#x}"),
                sub_op: SafeSubOp::call(ctf_addr, encode_set_approval_for_all(*op, true)),
            });
        }
    }

    if planned.is_empty() {
        bail!("no ops produced — check --asset / --spender combination");
    }

    // 4. Wrap in SafeTransaction. Single op = direct Call; >1 op = DelegateCall to
    //    MultiSend.
    let nonce = ctx.nonce().await?;
    let (safe_tx, op_label) = if planned.len() == 1 {
        let op = &planned[0];
        (
            SafeTransaction::call(op.sub_op.to, op.sub_op.data.clone(), nonce),
            "call",
        )
    } else {
        let multisend_addr = require_multisend(&ctx.cfg)?;
        let sub_ops: Vec<SafeSubOp> = planned.iter().map(|p| p.sub_op.clone()).collect();
        let packed = safe::multisend::encode(&sub_ops)
            .map_err(|e| anyhow!("multisend encode: {e}"))?;
        (
            SafeTransaction::delegate_call(multisend_addr, packed, nonce),
            "delegatecall(MultiSend)",
        )
    };

    // 5. Sign + assemble the SubmitRequest + plan JSON.
    let req = ctx.build_submit_request(&safe_tx, "approve")?;
    let ops_json: Vec<serde_json::Value> = planned
        .iter()
        .map(|p| serde_json::json!({ "summary": p.summary, "detail": p.detail }))
        .collect();
    let plan = safe_exec::assemble_plan(
        "predict-cli approve set",
        &ctx,
        op_label,
        nonce,
        ops_json,
        &req,
    );

    // 6. Dry-run prints + exits; execute submits + polls.
    if !a.execute {
        return safe_exec::print_plan(&plan, fmt, true, None);
    }

    let final_tx = ctx
        .submit_and_poll(
            &req,
            a.gamma_domain.as_deref(),
            a.gamma_uri.as_deref(),
            Duration::from_secs(a.poll_interval_secs.max(1)),
            Duration::from_secs(a.poll_timeout_secs.max(a.poll_interval_secs).max(5)),
        )
        .await?;

    safe_exec::print_plan(&plan, fmt, false, Some(safe_exec::final_state_json(&final_tx)))
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

    #[test]
    fn parse_amount_accepts_max_and_decimal_and_hex() {
        assert_eq!(parse_amount("MAX").unwrap(), U256::MAX);
        assert_eq!(parse_amount("max").unwrap(), U256::MAX);
        assert_eq!(parse_amount("uint256_max").unwrap(), U256::MAX);
        assert_eq!(parse_amount("1000000").unwrap(), U256::from(1_000_000u64));
        assert_eq!(parse_amount("0xff").unwrap(), U256::from(0xffu64));
        assert!(parse_amount("not-a-number").is_err());
    }

    #[test]
    fn encode_approve_matches_known_selector_and_layout() {
        let spender = Address::from_str("0x50b7B00EE75F8bFb5cDa892883aFb3867851c738").unwrap();
        let data = encode_approve(spender, U256::MAX);
        assert_eq!(data.len(), 4 + 32 + 32);
        assert_eq!(&data[..4], &ERC20_APPROVE_SELECTOR);
        // bytes 4..16 must be zero-padding for the address slot.
        assert!(data[4..16].iter().all(|b| *b == 0));
        // bytes 16..36 must equal the spender address.
        assert_eq!(&data[16..36], spender.as_slice());
        // amount = MAX → 32 bytes of 0xff.
        assert!(data[36..68].iter().all(|b| *b == 0xff));
    }

    #[test]
    fn resolve_spenders_explicit_override_returns_single_entry() {
        let cfg = network_config::load("../examples/networks/monad-hermestrade.yaml").unwrap();
        let out = resolve_spenders(
            &cfg,
            Some("0x017641abFa4264121237023f9Fe678BF00F60De8"),
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            format!("{:?}", out[0].1).to_lowercase(),
            "0x017641abfa4264121237023f9fe678bf00f60de8"
        );
    }

    #[test]
    fn resolve_spenders_yaml_default_lists_three_targets() {
        let cfg = network_config::load("../examples/networks/monad-hermestrade.yaml").unwrap();
        let out = resolve_spenders(&cfg, None).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].0, "CTF Exchange");
        assert_eq!(out[1].0, "Neg Risk CTF Exchange");
        assert_eq!(out[2].0, "Neg Risk Adapter");
    }

    #[test]
    fn require_collateral_returns_usdw_from_monad_yaml() {
        let cfg = network_config::load("../examples/networks/monad-hermestrade.yaml").unwrap();
        let (raw, addr) = require_collateral(&cfg).unwrap();
        assert_eq!(raw, "0xb7bD080Df56FA76ce6CA4fA737d47815f7F8e746");
        assert_eq!(
            format!("{addr:?}").to_lowercase(),
            "0xb7bd080df56fa76ce6ca4fa737d47815f7f8e746"
        );
    }

    #[test]
    fn require_multisend_returns_yaml_address() {
        let cfg = network_config::load("../examples/networks/monad-hermestrade.yaml").unwrap();
        let addr = require_multisend(&cfg).unwrap();
        assert_eq!(
            format!("{addr:?}").to_lowercase(),
            "0xa238cbeb142c10ef7ad8442c6d1f9e89e07e7761"
        );
    }

    #[test]
    fn require_conditional_tokens_returns_yaml_address() {
        let cfg = network_config::load("../examples/networks/monad-hermestrade.yaml").unwrap();
        let addr = require_conditional_tokens(&cfg).unwrap();
        // The Monad ConditionalTokens contract sourced from gamma /public-info.
        assert_eq!(
            format!("{addr:?}").to_lowercase(),
            "0xd77d550092ab455bd1b9071e4185ecbb6e8d6a2a"
        );
    }

    #[test]
    fn encode_set_approval_for_all_matches_selector_and_padding() {
        use alloy::primitives::keccak256;
        let operator = Address::from_str("0x017641abFa4264121237023f9Fe678BF00F60De8").unwrap();
        let data = encode_set_approval_for_all(operator, true);
        assert_eq!(data.len(), 4 + 32 + 32);
        // Selector golden-check against the literal signature.
        let want_selector = &keccak256("setApprovalForAll(address,bool)".as_bytes()).0[..4];
        assert_eq!(&data[..4], want_selector);
        assert_eq!(&data[..4], &ERC1155_SET_APPROVAL_FOR_ALL_SELECTOR);
        // Address slot padded left with 12 zero bytes.
        assert!(data[4..16].iter().all(|b| *b == 0));
        assert_eq!(&data[16..36], operator.as_slice());
        // Boolean true → 32 bytes with only the last byte = 0x01.
        assert!(data[36..67].iter().all(|b| *b == 0));
        assert_eq!(data[67], 1);

        // approved=false → all zeros after selector.
        let data_off = encode_set_approval_for_all(operator, false);
        assert!(data_off[36..68].iter().all(|b| *b == 0));
    }
}
