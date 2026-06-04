//! `predict-cli deposit` — USDW wrap (充值), and the withdraw (提现) helpers.
//!
//! Deposit is **EOA-side**: the EOA's USDC is approved to the tenant's `USDWrapper` and `wrap`ped,
//! minting USDW directly into the Safe. This is the one predict-cli flow that broadcasts a *direct*
//! EOA transaction — every other on-chain write goes through the Safe meta-tx relayer, but here the
//! USDC lives in the EOA, so there is no Safe to route through. `wrap` is immediate (single tx).
//!
//! Withdraw is Safe-side and two-step (handled elsewhere): `initiateUnwrap` burns the Safe's USDW
//! via the relayer, then after `unwrapDelay` `claimUnwrap` releases USDC to the Safe.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::network::EthereumWallet;
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use predict_rs_clob_client::safe::multisend::SafeSubOp;
use predict_rs_clob_client::safe::{self, SafeTransaction};

use crate::cli::Cli;
use crate::output::{self, Format};
use crate::safe_exec::{self, SafeContext};

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
        function decimals() external view returns (uint8);
    }

    #[sol(rpc)]
    interface IUSDWrapper {
        function wrap(address asset, uint256 assetAmount, address to) external returns (uint256 usdwAmount);
        function initiateUnwrap(uint256 usdwAmount, address asset) external returns (uint256 requestId);
        function claimUnwrap(uint256 requestId) external;
        function nextRequestId() external view returns (uint256);
        function minUnwrapUsdw() external view returns (uint256);
        function unwrapDelay() external view returns (uint256);
        function unwrapRequests(uint256 requestId) external view returns (address recipient, address asset, uint256 assetAmount, uint64 claimableAt, bool claimed);
    }
}

#[derive(Debug, Args)]
pub struct DepositArgs {
    /// Amount of the underlying asset (USDC) to deposit, in whole units (e.g. `5` or `5.5`).
    #[arg(long)]
    pub amount: String,
    /// Recipient of the minted USDW. Defaults to your Safe (`config.toml` `safe_address`).
    #[arg(long)]
    pub to: Option<String>,
    /// Underlying asset address (USDC). Defaults to the selected network's `contracts.usdc`.
    #[arg(long)]
    pub asset: Option<String>,
    /// Build + check (balance / allowance) but do NOT broadcast.
    #[arg(long)]
    pub dry_run: bool,
}

/// `predict-cli deposit` — approve (if needed) + `USDWrapper.wrap(USDC, amount, Safe)`, broadcast
/// directly from the EOA. Mints USDW into the Safe.
pub async fn run_deposit(args: &Cli, a: &DepositArgs, fmt: Format) -> Result<()> {
    let net = crate::networks::effective_network(args)?;
    let rpc_url = net.network.rpc_url.clone();

    let wrapper = parse_addr(
        net.contracts
            .usd_wrapper
            .as_deref()
            .ok_or_else(|| anyhow!("network '{}' has no contracts.usd_wrapper", net.network.name))?,
    )?;
    let asset_hex = a
        .asset
        .clone()
        .or_else(|| net.contracts.usdw_underlying.clone())
        .ok_or_else(|| {
            anyhow!(
                "no underlying USDC address: pass --asset <addr> or set contracts.usdw_underlying for network '{}'",
                net.network.name
            )
        })?;
    let asset = parse_addr(&asset_hex)?;
    let to = resolve_to(args, a.to.as_deref())?;

    let (pk, _src) = crate::wallet_commands::resolve_private_key(args)?;
    let signer = parse_signer(&pk)?;
    let eoa = signer.address();
    let provider = ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect_http(rpc_url.parse().with_context(|| format!("invalid rpc url {rpc_url}"))?);

    let usdc = IERC20::new(asset, &provider);
    let decimals = usdc.decimals().call().await.context("read asset decimals")?;
    let amount = parse_amount(&a.amount, decimals)?;
    if amount.is_zero() {
        bail!("deposit amount must be > 0");
    }

    let balance = usdc.balanceOf(eoa).call().await.context("read EOA asset balance")?;
    if !a.dry_run && balance < amount {
        bail!(
            "EOA {eoa:?} holds {} of {asset:?} but deposit needs {} (base units) — fund the EOA with USDC first",
            balance,
            amount
        );
    }
    let allowance = usdc.allowance(eoa, wrapper).call().await.context("read allowance")?;
    let needs_approve = allowance < amount;

    if a.dry_run || matches!(fmt, Format::Json) {
        let plan = serde_json::json!({
            "action": "deposit (wrap)",
            "eoa": format!("{eoa:?}"),
            "asset": format!("{asset:?}"),
            "wrapper": format!("{wrapper:?}"),
            "mint_usdw_to": format!("{to:?}"),
            "amount_base_units": amount.to_string(),
            "asset_decimals": decimals,
            "eoa_balance": balance.to_string(),
            "needs_approve": needs_approve,
            "dry_run": a.dry_run,
        });
        output::print_json(&plan)?;
        if a.dry_run {
            return Ok(());
        }
    } else {
        println!("deposit: wrap {} (base units) of {asset:?} → USDW to {to:?}", amount);
        println!("  eoa     : {eoa:?}");
        println!("  wrapper : {wrapper:?}");
        if needs_approve {
            println!("  approve : USDC → wrapper (allowance {allowance} < {amount})");
        }
    }

    if needs_approve {
        let receipt = usdc
            .approve(wrapper, amount)
            .send()
            .await
            .context("submit approve")?
            .get_receipt()
            .await
            .context("approve receipt")?;
        println!(
            "  approved: tx {:?} ({})",
            receipt.transaction_hash,
            if receipt.status() { "ok" } else { "REVERTED" }
        );
        if !receipt.status() {
            bail!("approve transaction reverted (tx {:?})", receipt.transaction_hash);
        }
    }

    let wrapper_c = IUSDWrapper::new(wrapper, &provider);
    let receipt = wrapper_c
        .wrap(asset, amount, to)
        .send()
        .await
        .context("submit wrap")?
        .get_receipt()
        .await
        .context("wrap receipt")?;

    let usdw = net
        .contracts
        .usdw
        .as_deref()
        .and_then(|h| parse_addr(h).ok());
    let safe_usdw = match usdw {
        Some(u) => IERC20::new(u, &provider).balanceOf(to).call().await.ok(),
        None => None,
    };

    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "wrap_tx": format!("{:?}", receipt.transaction_hash),
            "status": if receipt.status() { "success" } else { "reverted" },
            "safe": format!("{to:?}"),
            "safe_usdw_after": safe_usdw.map(|b| b.to_string()),
        }))?,
        Format::Table => {
            println!(
                "  wrapped : tx {:?} ({})",
                receipt.transaction_hash,
                if receipt.status() { "success" } else { "REVERTED" }
            );
            if let Some(b) = safe_usdw {
                println!("  safe USDW now: {b}");
            }
        }
    }
    if !receipt.status() {
        bail!("wrap transaction reverted");
    }
    Ok(())
}

// ─── withdraw (提现): initiateUnwrap (Safe relayer) + claimUnwrap (EOA, after delay) ─────────

#[derive(Debug, Subcommand)]
pub enum WithdrawCommand {
    /// Step 1: burn the Safe's USDW and open a delayed unwrap request (Safe meta-tx via relayer).
    Initiate(InitiateArgs),
    /// Step 2 (after `unwrapDelay`): release the USDC to the Safe. Permissionless — direct EOA tx.
    Claim(ClaimArgs),
    /// Read an unwrap request's on-chain state.
    Status(StatusArgs),
}

#[derive(Debug, Args)]
pub struct InitiateArgs {
    /// USDW amount to unwrap, in whole units (e.g. `1` or `1.5`).
    #[arg(long)]
    pub amount: String,
    /// Underlying asset to receive (USDC). Defaults to the network's `contracts.usdw_underlying`.
    #[arg(long)]
    pub asset: Option<String>,
    /// Override the network RPC URL.
    #[arg(long)]
    pub rpc_url: Option<String>,
    /// Sign + assemble but do NOT submit to the relayer.
    #[arg(long)]
    pub dry_run: bool,
    /// Relayer poll interval (seconds).
    #[arg(long, default_value_t = 2)]
    pub poll_interval_secs: u64,
    /// Relayer poll timeout (seconds).
    #[arg(long, default_value_t = 120)]
    pub poll_timeout_secs: u64,
}

#[derive(Debug, Args)]
pub struct ClaimArgs {
    /// Request id from `withdraw initiate` (decimal, or `0x`-hex).
    #[arg(long)]
    pub request_id: String,
    /// Override the network RPC URL.
    #[arg(long)]
    pub rpc_url: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Request id (decimal, or `0x`-hex).
    #[arg(long)]
    pub request_id: String,
    /// Override the network RPC URL.
    #[arg(long)]
    pub rpc_url: Option<String>,
}

pub async fn run_withdraw(args: &Cli, cmd: &WithdrawCommand, fmt: Format) -> Result<()> {
    match cmd {
        WithdrawCommand::Initiate(a) => run_initiate(args, a, fmt).await,
        WithdrawCommand::Claim(a) => run_claim(args, a, fmt).await,
        WithdrawCommand::Status(a) => run_status(args, a, fmt).await,
    }
}

async fn run_initiate(args: &Cli, a: &InitiateArgs, fmt: Format) -> Result<()> {
    let net = crate::networks::effective_network(args)?;
    let ctx = SafeContext::resolve(args, net, a.rpc_url.as_deref())?;
    let wrapper = parse_addr(ctx.cfg.contracts.usd_wrapper.as_deref().ok_or_else(|| {
        anyhow!("network '{}' has no contracts.usd_wrapper", ctx.cfg.network.name)
    })?)?;
    let usdw = parse_addr(ctx.cfg.contracts.usdw.as_deref().ok_or_else(|| {
        anyhow!("network '{}' has no contracts.usdw", ctx.cfg.network.name)
    })?)?;
    let asset = parse_addr(
        &a.asset
            .clone()
            .or_else(|| ctx.cfg.contracts.usdw_underlying.clone())
            .ok_or_else(|| anyhow!("no underlying asset: pass --asset or set contracts.usdw_underlying"))?,
    )?;
    let amount = parse_amount(&a.amount, 6)?; // USDW is 6 decimals
    if amount.is_zero() {
        bail!("amount must be > 0");
    }

    // Read-only pre-flight: min, delay, the request id this initiate will receive, Safe USDW.
    let ro = ProviderBuilder::new().connect_http(ctx.rpc_url.parse()?);
    let w = IUSDWrapper::new(wrapper, &ro);
    let min = w.minUnwrapUsdw().call().await.context("read minUnwrapUsdw")?;
    if amount < min {
        bail!("amount {amount} (base units) is below minUnwrapUsdw {min}");
    }
    let delay = w.unwrapDelay().call().await.unwrap_or(U256::ZERO);
    let request_id = w.nextRequestId().call().await.context("read nextRequestId")?;
    let safe_usdw = IERC20::new(usdw, &ro)
        .balanceOf(ctx.safe)
        .call()
        .await
        .unwrap_or(U256::ZERO);
    if !a.dry_run && safe_usdw < amount {
        bail!("Safe {:?} holds {safe_usdw} USDW < {amount}", ctx.safe);
    }

    // MultiSend: USDW.approve(wrapper, amount) then USDWrapper.initiateUnwrap(amount, asset).
    let approve_data = IERC20::approveCall { spender: wrapper, amount }.abi_encode();
    let initiate_data = IUSDWrapper::initiateUnwrapCall { usdwAmount: amount, asset }.abi_encode();
    let sub_ops = vec![
        SafeSubOp::call(usdw, approve_data),
        SafeSubOp::call(wrapper, initiate_data),
    ];
    let nonce = ctx.nonce().await?;
    let multisend = parse_addr(
        ctx.cfg
            .security
            .as_ref()
            .and_then(|s| s.multi_send_address.as_deref())
            .ok_or_else(|| anyhow!("network '{}' has no security.multi_send_address", ctx.cfg.network.name))?,
    )?;
    let packed = safe::multisend::encode(&sub_ops).map_err(|e| anyhow!("multisend encode: {e}"))?;
    let safe_tx = SafeTransaction::delegate_call(multisend, packed, nonce);
    let req = ctx.build_submit_request(&safe_tx, "unwrap")?;

    let ops_json = vec![
        serde_json::json!({
            "summary": format!("USDW.approve → wrapper {wrapper:#x}"),
            "detail": format!("approve(amount={amount})"),
        }),
        serde_json::json!({
            "summary": format!("USDWrapper.initiateUnwrap → {wrapper:#x}"),
            "detail": format!("initiateUnwrap(usdw={amount}, asset={asset:#x}); requestId={request_id}"),
        }),
    ];
    let plan = safe_exec::assemble_plan(
        "withdraw initiate (burn Safe USDW, open delayed unwrap)",
        &ctx,
        "delegatecall(MultiSend)",
        nonce,
        ops_json,
        &req,
    );

    let hours = delay / U256::from(3600u64);
    println!("request_id (this initiate): {request_id}");
    println!("after submit, claimable in ~{delay}s (~{hours}h) via: predict-cli withdraw claim --request-id {request_id}");

    if a.dry_run {
        return safe_exec::print_plan(&plan, fmt, true, None);
    }
    let final_tx = ctx
        .submit_and_poll(
            &req,
            None,
            None,
            Duration::from_secs(a.poll_interval_secs.max(1)),
            Duration::from_secs(a.poll_timeout_secs.max(5)),
        )
        .await?;
    safe_exec::print_plan(&plan, fmt, false, Some(safe_exec::final_state_json(&final_tx)))
}

async fn run_claim(args: &Cli, a: &ClaimArgs, fmt: Format) -> Result<()> {
    let net = crate::networks::effective_network(args)?;
    let rpc = a.rpc_url.clone().unwrap_or_else(|| net.network.rpc_url.clone());
    let wrapper = parse_addr(net.contracts.usd_wrapper.as_deref().ok_or_else(|| {
        anyhow!("network '{}' has no contracts.usd_wrapper", net.network.name)
    })?)?;
    let request_id = parse_u256(&a.request_id)?;

    let ro = ProviderBuilder::new().connect_http(rpc.parse()?);
    let r = IUSDWrapper::new(wrapper, &ro)
        .unwrapRequests(request_id)
        .call()
        .await
        .context("read unwrap request")?;
    if r.assetAmount.is_zero() {
        bail!("unwrap request {request_id} not found");
    }
    if r.claimed {
        bail!("unwrap request {request_id} already claimed");
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let claimable_at: u64 = r.claimableAt;
    if claimable_at > now {
        bail!(
            "request {request_id} not claimable yet — claimable at unix {claimable_at} (~{}s left)",
            claimable_at.saturating_sub(now)
        );
    }

    let (pk, _src) = crate::wallet_commands::resolve_private_key(args)?;
    let signer = parse_signer(&pk)?;
    let provider = ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect_http(rpc.parse()?);
    let receipt = IUSDWrapper::new(wrapper, &provider)
        .claimUnwrap(request_id)
        .send()
        .await
        .context("submit claimUnwrap")?
        .get_receipt()
        .await
        .context("claim receipt")?;
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "claim_tx": format!("{:?}", receipt.transaction_hash),
            "status": if receipt.status() { "success" } else { "reverted" },
            "recipient": format!("{:?}", r.recipient),
            "asset": format!("{:?}", r.asset),
            "asset_amount": r.assetAmount.to_string(),
        }))?,
        Format::Table => println!(
            "claimed: tx {:?} ({}) — {} of {:?} → {:?}",
            receipt.transaction_hash,
            if receipt.status() { "success" } else { "REVERTED" },
            r.assetAmount,
            r.asset,
            r.recipient,
        ),
    }
    if !receipt.status() {
        bail!("claimUnwrap reverted");
    }
    Ok(())
}

async fn run_status(args: &Cli, a: &StatusArgs, fmt: Format) -> Result<()> {
    let net = crate::networks::effective_network(args)?;
    let rpc = a.rpc_url.clone().unwrap_or_else(|| net.network.rpc_url.clone());
    let wrapper = parse_addr(net.contracts.usd_wrapper.as_deref().ok_or_else(|| {
        anyhow!("network '{}' has no contracts.usd_wrapper", net.network.name)
    })?)?;
    let request_id = parse_u256(&a.request_id)?;
    let ro = ProviderBuilder::new().connect_http(rpc.parse()?);
    let r = IUSDWrapper::new(wrapper, &ro)
        .unwrapRequests(request_id)
        .call()
        .await
        .context("read unwrap request")?;
    if r.assetAmount.is_zero() {
        bail!("unwrap request {request_id} not found");
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let claimable_now = !r.claimed && r.claimableAt <= now;
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "request_id": request_id.to_string(),
            "recipient": format!("{:?}", r.recipient),
            "asset": format!("{:?}", r.asset),
            "asset_amount": r.assetAmount.to_string(),
            "claimable_at": r.claimableAt,
            "claimed": r.claimed,
            "claimable_now": claimable_now,
        }))?,
        Format::Table => {
            println!("request      : {request_id}");
            println!("recipient    : {:?}", r.recipient);
            println!("asset        : {:?}", r.asset);
            println!("asset_amount : {}", r.assetAmount);
            println!("claimable_at : {} (unix)", r.claimableAt);
            println!("claimed      : {}", r.claimed);
            println!("claimable_now: {claimable_now}");
        }
    }
    Ok(())
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn parse_u256(s: &str) -> Result<U256> {
    let s = s.trim();
    let v = if let Some(hex) = s.strip_prefix("0x") {
        U256::from_str_radix(hex, 16)
    } else {
        U256::from_str_radix(s, 10)
    };
    v.map_err(|e| anyhow!("invalid u256 '{s}': {e}"))
}

fn resolve_to(args: &Cli, flag: Option<&str>) -> Result<Address> {
    if let Some(s) = flag {
        return parse_addr(s);
    }
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let safe = stored
        .and_then(|c| c.safe_address)
        .ok_or_else(|| {
            anyhow!("no recipient: pass --to <addr> or store a Safe (`predict-cli wallet set-safe <addr>`)")
        })?;
    parse_addr(&safe)
}

fn parse_addr(s: &str) -> Result<Address> {
    s.trim()
        .parse::<Address>()
        .map_err(|e| anyhow!("invalid address '{s}': {e}"))
}

fn parse_signer(hex_str: &str) -> Result<alloy::signers::local::PrivateKeySigner> {
    let clean = hex_str.trim().trim_start_matches("0x");
    let bytes = hex::decode(clean).map_err(|e| anyhow!("invalid private key hex: {e}"))?;
    if bytes.len() != 32 {
        bail!("private key must be 32 bytes, got {}", bytes.len());
    }
    let buf: [u8; 32] = bytes.try_into().expect("checked len 32");
    alloy::signers::local::PrivateKeySigner::from_bytes(&buf.into())
        .map_err(|e| anyhow!("invalid private key: {e}"))
}

/// Parse a decimal whole-unit amount (e.g. `"5.5"`) into base units given the asset's `decimals`.
fn parse_amount(s: &str, decimals: u8) -> Result<U256> {
    let s = s.trim();
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if frac_part.len() > decimals as usize {
        bail!("amount '{s}' has more decimal places than the asset supports ({decimals})");
    }
    let mut digits = String::new();
    digits.push_str(if int_part.is_empty() { "0" } else { int_part });
    digits.push_str(frac_part);
    for _ in 0..(decimals as usize - frac_part.len()) {
        digits.push('0');
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        bail!("invalid amount '{s}'");
    }
    U256::from_str_radix(&digits, 10).map_err(|e| anyhow!("invalid amount '{s}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_scales_by_decimals() {
        assert_eq!(parse_amount("5", 6).unwrap(), U256::from(5_000_000u64));
        assert_eq!(parse_amount("5.5", 6).unwrap(), U256::from(5_500_000u64));
        assert_eq!(parse_amount("0.001", 6).unwrap(), U256::from(1_000u64));
        assert_eq!(parse_amount("0", 6).unwrap(), U256::ZERO);
    }

    #[test]
    fn amount_rejects_excess_precision() {
        assert!(parse_amount("1.1234567", 6).is_err());
    }
}
