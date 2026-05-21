//! `pm approve` subcommands.
//!
//! - `check` reads `IERC20.allowance(owner, spender)` and `IERC1155.isApprovedForAll`
//!   per the tenant YAML's approval targets. No on-chain writes.
//! - `set` writes `usdw.approve(spender, amount)` through the user's Safe, using the
//!   chainup `relayer-service` to pay gas. Defaults to dry-run; `--execute` actually
//!   submits. Safe-mode (`signatureType=2`) only — chainup does not support EOA mode.
//!
//! `owner` for `check` defaults to the EOA derived from the configured wallet, but
//! **chainup users by default trade through a Safe** (signatureType=2). The Safe is the
//! address holding USDW and CTF balances. Pass `--address <safe>` when checking a Safe
//! owner — for `set`, the Safe address comes from the local config and the EOA is the
//! Safe owner that signs the `SafeTx` EIP-712 payload.

use std::str::FromStr;
use std::time::Duration;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use pm_rs_clob_client::relayer::{SafeTxParams, SubmitRequest, SubmitType};
use pm_rs_clob_client::safe::multisend::SafeSubOp;
use pm_rs_clob_client::safe::{self, SafeTransaction};
use pm_rs_clob_client::types::ScopeId;
use pm_rs_clob_client::{Client, Endpoints, PMCup26Signer};
use url::Url;

use crate::cli::Cli;
use crate::network_config::{self, ApprovalTarget, NetworkConfig};
use crate::output::{self, Format};

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

    #[sol(rpc)]
    interface ISafe {
        function nonce() external view returns (uint256);
    }
}

#[derive(Debug, Subcommand)]
pub enum ApproveCommand {
    /// Read `USDW.allowance(owner, target)` and `CTF.isApprovedForAll(owner, target)` for
    /// each tenant approval target. No on-chain writes.
    Check(CheckArgs),
    /// Safe-mode `usdw.approve(spender, amount)` via the chainup relayer-service. Default
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
    /// **For Safe-wallet users (`signatureType=2`, chainup default) you must pass the Safe
    /// address explicitly here** — the EOA holds no funds and its allowance is always zero.
    #[arg(long)]
    pub address: Option<String>,
    /// Override the RPC URL from the network config (e.g. for a fork node).
    #[arg(long)]
    pub rpc_url: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// Path to the tenant network YAML. Schema matches `examples/networks/*.yaml`.
    #[arg(long)]
    pub network_config: String,
    /// Single spender to approve. Default: every entry returned by `approval_targets()`
    /// (CtfExchange + NegRiskCtfExchange + NegRiskAdapter) bundled into one MultiSend.
    #[arg(long)]
    pub spender: Option<String>,
    /// Amount in raw smallest unit. Use `MAX` for `uint256::MAX`. Default `MAX`.
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
    // for backward compatibility with older YAMLs (and chainup Monad where the two are the
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

// ─── pm approve set ─────────────────────────────────────────────────────

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

fn resolve_safe_address(args: &Cli) -> Result<Address> {
    let stored = crate::config_store::load(args.config_dir.as_deref())?
        .ok_or_else(|| anyhow!("no local config — run `pm wallet create` / `pm wallet set-safe <addr>` first"))?;
    let raw = stored.safe_address.ok_or_else(|| {
        anyhow!(
            "no Safe address configured: run `pm wallet set-safe <addr>` (manual) or `pm wallet detect-safe` (server)"
        )
    })?;
    Address::from_str(&raw).with_context(|| format!("invalid stored safe_address '{raw}'"))
}

fn require_signature_type_safe(args: &Cli) -> Result<()> {
    let st = crate::commands::effective_signature_type(args)?;
    if !matches!(st, pm_rs_clob_client::types::SignatureType::PolyGnosisSafe) {
        bail!(
            "`pm approve set` requires signatureType=gnosis-safe; current = {st:?}. chainup only supports Safe-mode writes"
        );
    }
    Ok(())
}

fn build_signer(args: &Cli, cfg: &NetworkConfig) -> Result<PMCup26Signer> {
    let (pk, _source) = crate::wallet_commands::resolve_private_key(args)?;
    let mut signer = PMCup26Signer::from_hex(&pk, cfg.network.chain_id)
        .with_context(|| "invalid private key")?;
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let scope_hex = if !args.scope_id.is_empty() {
        args.scope_id.clone()
    } else {
        stored.and_then(|c| c.scope_id).unwrap_or_default()
    };
    if !scope_hex.is_empty() {
        let scope = ScopeId::from_hex(&scope_hex)
            .with_context(|| format!("invalid scope id '{scope_hex}'"))?;
        signer = signer.with_scope_id(scope);
    }
    Ok(signer)
}

fn build_endpoints(cfg: &NetworkConfig) -> Result<Endpoints> {
    let clob = with_slash(&cfg.tenant.endpoints.clob);
    // Start from the explicit CLOB URL; gamma / relayer come from YAML (no canonical
    // subdomain assumption — production deployments often use `relayer.<host>` rather
    // than `relayer-api.<host>`).
    let mut ep = Endpoints::clob_only(&clob)
        .with_context(|| format!("invalid clob endpoint '{clob}'"))?;
    if let Some(gamma) = cfg.tenant.endpoints.gamma.as_deref() {
        let url = Url::parse(&with_slash(gamma))
            .with_context(|| format!("invalid gamma endpoint '{gamma}'"))?;
        ep = ep.with_gamma(url);
    }
    if let Some(rel) = cfg.tenant.endpoints.relayer.as_deref() {
        let url = Url::parse(&with_slash(rel))
            .with_context(|| format!("invalid relayer endpoint '{rel}'"))?;
        ep = ep.with_relayer(url);
    } else {
        bail!(
            "network config has no `tenant.endpoints.relayer` — required for `pm approve set`"
        );
    }
    Ok(ep)
}

fn with_slash(s: &str) -> String {
    if s.ends_with('/') { s.to_owned() } else { format!("{s}/") }
}

/// Read the Safe's current `nonce()` via JSON-RPC.
async fn read_safe_nonce(rpc_url: &str, safe: Address) -> Result<U256> {
    let url = Url::parse(rpc_url).with_context(|| format!("invalid rpc_url '{rpc_url}'"))?;
    let provider = ProviderBuilder::new().connect_http(url);
    let safe_view = ISafe::new(safe, provider);
    let nonce = safe_view
        .nonce()
        .call()
        .await
        .with_context(|| format!("read Safe.nonce() at {safe:?}"))?;
    Ok(nonce)
}

async fn run_set(args: &Cli, a: &SetArgs, fmt: Format) -> Result<()> {
    let cfg = network_config::load(&a.network_config)?;

    // 0. Sanity gates first — fail fast before any RPC / signing work.
    require_signature_type_safe(args)?;

    // 1. Resolve wallet identity.
    let safe = resolve_safe_address(args)?;
    let signer = build_signer(args, &cfg)?;
    let eoa = signer.address();

    // 2. Resolve assets + spenders.
    let (usdw_raw, usdw) = require_collateral(&cfg)?;
    let amount = parse_amount(&a.amount)?;
    let spenders = resolve_spenders(&cfg, a.spender.as_deref())?;
    if spenders.is_empty() {
        bail!("no spenders to approve");
    }

    // 3. Build the SafeTransaction calldata. One spender = single Call;
    //    N spenders = MultiSend DelegateCall.
    let (safe_to, safe_data, operation_delegatecall) = if spenders.len() == 1 {
        let (_name, sp) = &spenders[0];
        let data = encode_approve(*sp, amount);
        (usdw, data, false)
    } else {
        let multisend_addr = require_multisend(&cfg)?;
        let ops: Vec<SafeSubOp> = spenders
            .iter()
            .map(|(_name, sp)| SafeSubOp::call(usdw, encode_approve(*sp, amount)))
            .collect();
        let packed = safe::multisend::encode(&ops)
            .map_err(|e| anyhow!("multisend encode: {e}"))?;
        (multisend_addr, packed, true)
    };

    // 4. Read Safe nonce.
    let rpc_url = a.rpc_url.clone().unwrap_or_else(|| cfg.network.rpc_url.clone());
    let nonce = read_safe_nonce(&rpc_url, safe).await?;

    let safe_tx = if operation_delegatecall {
        SafeTransaction::delegate_call(safe_to, safe_data.clone(), nonce)
    } else {
        SafeTransaction::call(safe_to, safe_data.clone(), nonce)
    };

    // 5. Sign locally.
    let signature = signer.sign_safe_tx(safe, &safe_tx)
        .with_context(|| "sign SafeTx")?;
    let signature_hex = format!("0x{}", hex::encode(signature));

    // 6. Build the SubmitRequest body.
    let scope_id_hex = if signer.scope_id().is_zero() {
        None
    } else {
        Some(format!("{:#x}", signer.scope_id().as_b256()))
    };
    let req = SubmitRequest {
        from: format!("{eoa:#x}"),
        to: format!("{safe_to:#x}"),
        proxy_wallet: format!("{safe:#x}"),
        data: format!("0x{}", hex::encode(&safe_data)),
        nonce: Some(nonce.to_string()),
        signature: signature_hex.clone(),
        signature_params: serde_json::to_value(SafeTxParams::relayer_pays(
            operation_delegatecall,
        ))?,
        r#type: SubmitType::Safe,
        scope_id: scope_id_hex,
        metadata: Some("approve".to_owned()),
    };

    let plan_json = build_plan_json(&cfg, eoa, safe, &usdw_raw, &spenders, amount, nonce, operation_delegatecall, &req)?;

    // 7. Branch: dry-run prints the plan and exits; execute submits + polls.
    if !a.execute {
        print_plan(&plan_json, fmt, true, None)?;
        return Ok(());
    }

    // 8. Real submit — needs the relayer URL and a JWT.
    let endpoints = build_endpoints(&cfg)?;
    let client = Client::builder()
        .endpoints(endpoints)
        .chain_id(cfg.network.chain_id)
        .build()
        .context("build client")?;

    let domain = a
        .gamma_domain
        .clone()
        .unwrap_or_else(|| cfg.tenant.domain.clone());
    let uri = a
        .gamma_uri
        .clone()
        .unwrap_or_else(|| format!("https://{}", cfg.tenant.domain));

    let jwt = client.jwt_login(&signer, domain.clone(), uri.clone()).await
        .with_context(|| format!("jwt_login(domain={domain}, uri={uri})"))?;

    let relayer = client.relayer()?.with_token(&jwt);
    let resp = relayer.submit(&req).await.with_context(|| "relayer submit")?;

    let final_tx = relayer
        .poll_until_terminal(
            &resp.transaction_id,
            Duration::from_secs(a.poll_interval_secs.max(1)),
            Duration::from_secs(a.poll_timeout_secs.max(a.poll_interval_secs).max(5)),
        )
        .await
        .with_context(|| format!("poll relayer tx {}", resp.transaction_id))?;

    print_plan(
        &plan_json,
        fmt,
        false,
        Some(serde_json::json!({
            "transaction_id": final_tx.transaction_id,
            "transaction_hash": final_tx.transaction_hash,
            "state": final_tx.state,
            "block_number": final_tx.block_number,
            "gas_used": final_tx.gas_used,
            "error": final_tx.error,
        })),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_plan_json(
    cfg: &NetworkConfig,
    eoa: Address,
    safe: Address,
    usdw_raw: &str,
    spenders: &[(String, Address)],
    amount: U256,
    nonce: U256,
    delegatecall: bool,
    req: &SubmitRequest,
) -> Result<serde_json::Value> {
    let amount_label = if amount == U256::MAX {
        "MAX".to_owned()
    } else {
        amount.to_string()
    };
    let ops: Vec<serde_json::Value> = spenders
        .iter()
        .map(|(name, addr)| {
            serde_json::json!({
                "name": name,
                "spender": format!("{addr:#x}"),
                "asset": usdw_raw,
                "call": "approve(address,uint256)",
                "amount": amount_label,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "tenant": cfg.tenant.name,
        "chain": {
            "name": cfg.network.name,
            "chain_id": cfg.network.chain_id,
            "rpc": cfg.network.rpc_url,
        },
        "wallet": {
            "eoa": format!("{eoa:#x}"),
            "safe": format!("{safe:#x}"),
        },
        "operation": if delegatecall { "delegatecall(MultiSend)" } else { "call" },
        "safe_nonce": nonce.to_string(),
        "ops": ops,
        "submit_request": req,
    }))
}

fn print_plan(
    plan: &serde_json::Value,
    fmt: Format,
    dry_run: bool,
    final_state: Option<serde_json::Value>,
) -> Result<()> {
    match fmt {
        Format::Json => {
            let mut full = plan.clone();
            full.as_object_mut().unwrap().insert(
                "dry_run".into(),
                serde_json::Value::Bool(dry_run),
            );
            if let Some(f) = &final_state {
                full.as_object_mut().unwrap().insert("relayer_result".into(), f.clone());
            }
            output::print_json(&full)
        }
        Format::Table => {
            if dry_run {
                println!("== pm approve set (DRY-RUN — nothing was submitted) ==");
            } else {
                println!("== pm approve set ==");
            }
            println!("tenant : {}", plan["tenant"].as_str().unwrap_or("?"));
            println!(
                "chain  : {} (chainId {})",
                plan["chain"]["name"].as_str().unwrap_or("?"),
                plan["chain"]["chain_id"].as_i64().unwrap_or(0)
            );
            println!("eoa    : {}", plan["wallet"]["eoa"].as_str().unwrap_or("?"));
            println!("safe   : {}", plan["wallet"]["safe"].as_str().unwrap_or("?"));
            println!(
                "op     : {} @ nonce {}",
                plan["operation"].as_str().unwrap_or("?"),
                plan["safe_nonce"].as_str().unwrap_or("?")
            );
            for (i, op) in plan["ops"].as_array().into_iter().flatten().enumerate() {
                println!(
                    "  [{i}] {} → {} amount={}",
                    op["name"].as_str().unwrap_or("?"),
                    op["spender"].as_str().unwrap_or("?"),
                    op["amount"].as_str().unwrap_or("?"),
                );
            }
            println!(
                "signature: {}",
                plan["submit_request"]["signature"].as_str().unwrap_or("?")
            );
            if dry_run {
                println!("(re-run with --execute to actually submit)");
            } else if let Some(f) = &final_state {
                println!();
                println!("relayer  : id={}", f["transaction_id"].as_str().unwrap_or("?"));
                println!("           state={:?}", f["state"]);
                if let Some(h) = f["transaction_hash"].as_str()
                    && !h.is_empty()
                {
                    println!("           hash={h}");
                }
                if let Some(bn) = f["block_number"].as_u64() {
                    println!("           block={bn}");
                }
                if let Some(err) = f["error"].as_str() {
                    println!("           error={err}");
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
    fn build_endpoints_pulls_relayer_from_yaml_not_canonical_subdomain() {
        let cfg = network_config::load("../examples/networks/monad-hermestrade.yaml").unwrap();
        let ep = build_endpoints(&cfg).unwrap();
        // YAML overrides the `relayer-api.<host>` default with `relayer.<host>`.
        assert_eq!(
            ep.relayer.as_ref().unwrap().as_str(),
            "https://relayer.hermestrade.xyz/"
        );
        assert_eq!(ep.clob.as_str(), "https://clob-api.hermestrade.xyz/");
        assert_eq!(ep.gamma.as_ref().unwrap().as_str(), "https://gamma-api.hermestrade.xyz/");
    }
}
