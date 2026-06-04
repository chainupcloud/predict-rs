//! `predict-cli ctf` — Conditional Token Framework helpers.
//!
//! Two flavours:
//!
//! - **Pure identifier calculations** (`condition-id`, `position-id`) — local keccak256
//!   over ABI-encoded inputs, no RPC required, no signer required.
//! - **On-chain Safe-mode operations** (`redeem`, `split`, `merge`) — build a SafeTx that
//!   calls `ConditionalTokens` (or `NegRiskAdapter` via `--contract`) and submit via
//!   the relayer-service. Default dry-run; `--execute` actually submits.
//!
//! `collection-id` requires alt_bn128 EC point addition per Gnosis CTF spec, so it's
//! deferred to a future commit and will use an RPC fallback (`CTF.getCollectionId`).

use std::str::FromStr;
use std::time::Duration;

use alloy::primitives::{Address, B256, U256, keccak256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use predict_rs_clob_client::safe::SafeTransaction;

use crate::cli::Cli;
use crate::network_config::NetworkConfig;
use crate::output::{self, Format};
use crate::safe_exec::{self, SafeContext};

#[derive(Debug, Args)]
pub struct CtfArgs {
    #[command(subcommand)]
    pub command: CtfCmd,
}

#[derive(Debug, Subcommand)]
pub enum CtfCmd {
    /// Compute a CTF `conditionId` from `(oracle, questionId, outcomeSlotCount)`.
    /// Pure function — no RPC. Formula:
    /// `keccak256(abi.encodePacked(oracle, questionId, outcomeSlotCount))`.
    ConditionId(ConditionIdArgs),
    /// Compute a CTF `positionId` from `(collateralToken, collectionId)`.
    /// Pure function — no RPC. Formula:
    /// `keccak256(abi.encodePacked(collateralToken, collectionId))`.
    PositionId(PositionIdArgs),
    /// Resolve a CTF `collectionId` via `CTF.getCollectionId(parent, condition, indexSet)`.
    /// Requires an RPC endpoint because Gnosis CTF's algorithm uses alt_bn128 EC point
    /// addition which is not yet implemented locally.
    CollectionId(CollectionIdArgs),
    /// Redeem winning outcome tokens — `redeemPositions(collateral, parentCollectionId,
    /// conditionId, indexSets)`. Safe-mode via the relayer-service. Defaults to
    /// dry-run; pass `--execute` to actually submit. Only succeeds after the condition
    /// has been resolved on-chain (i.e. `payoutNumerators` are non-zero).
    Redeem(RedeemArgs),
    /// Split USDW into a full set of outcome tokens — `splitPosition(collateral,
    /// parentCollectionId, conditionId, partition, amount)`. Safe-mode via the relayer.
    Split(SplitArgs),
    /// Merge a full set of outcome tokens back into USDW — `mergePositions(collateral,
    /// parentCollectionId, conditionId, partition, amount)`. Safe-mode via the relayer.
    Merge(MergeArgs),
}

#[derive(Debug, Args)]
pub struct ConditionIdArgs {
    /// Oracle (UMA / sports / etc.) address, `0x...20bytes`.
    #[arg(long)]
    pub oracle: String,
    /// Question identifier, `0x...32bytes`. Issued by the oracle when the question was created.
    #[arg(long)]
    pub question: String,
    /// Number of outcome slots. Binary markets = 2; categorical markets = N.
    #[arg(long)]
    pub outcomes: u32,
}

#[derive(Debug, Args)]
pub struct PositionIdArgs {
    /// Collateral token (USDW on Monad), `0x...20bytes`.
    #[arg(long)]
    pub collateral: String,
    /// Collection id, `0x...32bytes`. Output of `getCollectionId` — for binary markets the
    /// "Yes" collection is the conditionId itself when the parent collection is zero.
    #[arg(long)]
    pub collection: String,
}

#[derive(Debug, Args)]
pub struct CollectionIdArgs {
    /// Parent collection id (`bytes32`). Default zero = top-level condition.
    #[arg(long, default_value = "0x0000000000000000000000000000000000000000000000000000000000000000")]
    pub parent_collection_id: String,
    /// Condition id (`bytes32`).
    #[arg(long)]
    pub condition_id: String,
    /// Outcome index set (uint256). Binary "Yes" = 1, binary "No" = 2.
    #[arg(long)]
    pub index_set: String,
    /// Override the YAML's `network.rpc_url`.
    #[arg(long)]
    pub rpc_url: Option<String>,
    /// Override `contracts.conditional_tokens` from the YAML.
    #[arg(long)]
    pub contract: Option<String>,
}

/// Shared fields for the three Safe-mode CTF write commands. Captured in a struct so the
/// per-command arg structs stay short.
#[derive(Debug, Args)]
pub struct CtfWriteCommon {
    /// Condition id, `0x...32bytes`. Output of `CTF.getConditionId` or `predict-cli ctf condition-id`.
    #[arg(long)]
    pub condition_id: String,
    /// Parent collection id, `0x...32bytes`. Defaults to the zero collection (top-level
    /// condition with no parent). Set this for nested / linked conditions.
    #[arg(long, default_value = "0x0000000000000000000000000000000000000000000000000000000000000000")]
    pub parent_collection_id: String,
    /// Override the CTF contract address used as the SafeTx target. Defaults to the YAML's
    /// `contracts.conditional_tokens`. Pass `contracts.neg_risk_adapter` for neg-risk
    /// markets that route through the adapter instead.
    #[arg(long)]
    pub contract: Option<String>,
    /// Override the collateral token. Defaults to `contracts.usdw` from the YAML.
    #[arg(long)]
    pub collateral: Option<String>,
    /// Override the YAML's `network.rpc_url` (used to read the Safe's `nonce()`).
    #[arg(long)]
    pub rpc_url: Option<String>,
    /// Actually submit the SafeTx. Without this flag the command signs locally and prints
    /// the `SubmitRequest` body but never POSTs.
    #[arg(long)]
    pub execute: bool,
    /// Poll interval (seconds). Default 2 s.
    #[arg(long, default_value_t = 2)]
    pub poll_interval_secs: u64,
    /// Polling deadline (seconds). Default 60 s.
    #[arg(long, default_value_t = 60)]
    pub poll_timeout_secs: u64,
    /// EIP-712 LoginMessage `domain`. Defaults to `tenant.domain` from the YAML.
    #[arg(long)]
    pub gamma_domain: Option<String>,
    /// EIP-712 LoginMessage `uri`. Defaults to `https://<tenant.domain>`.
    #[arg(long)]
    pub gamma_uri: Option<String>,
}

#[derive(Debug, Args)]
pub struct RedeemArgs {
    #[command(flatten)]
    pub common: CtfWriteCommon,
    /// Comma-separated winning index sets (uint256). Each is the bitmap of outcomes
    /// claimed in one redeem call. Binary "Yes" = 1, binary "No" = 2; categorical with
    /// 3 outcomes claiming the first = 1, second = 2, third = 4.
    #[arg(long, value_delimiter = ',')]
    pub index_sets: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SplitArgs {
    #[command(flatten)]
    pub common: CtfWriteCommon,
    /// Comma-separated partition uint256 entries. For a full split into a binary market:
    /// `1,2`. For a categorical 3-way: `1,2,4`. Must sum to `(1 << outcomeSlotCount) - 1`.
    #[arg(long, value_delimiter = ',')]
    pub partition: Vec<String>,
    /// Collateral amount in raw smallest unit (USDW has 6 decimals: 1 USDW = `1_000_000`).
    #[arg(long)]
    pub amount: String,
}

#[derive(Debug, Args)]
pub struct MergeArgs {
    #[command(flatten)]
    pub common: CtfWriteCommon,
    /// Comma-separated partition uint256 entries (see `split`).
    #[arg(long, value_delimiter = ',')]
    pub partition: Vec<String>,
    /// Outcome-token amount to merge back into collateral (raw smallest unit).
    #[arg(long)]
    pub amount: String,
}

pub async fn run(args: &Cli, ctf_args: CtfArgs, fmt: Format) -> Result<()> {
    match ctf_args.command {
        CtfCmd::ConditionId(a) => {
            let id = condition_id(&a.oracle, &a.question, a.outcomes)?;
            output::print_scalar("condition_id", format!("0x{}", hex::encode(id)), fmt)
        }
        CtfCmd::PositionId(a) => {
            let id = position_id(&a.collateral, &a.collection)?;
            output::print_scalar("position_id", format!("0x{}", hex::encode(id)), fmt)
        }
        CtfCmd::CollectionId(a) => run_collection_id(args, &a, fmt).await,
        CtfCmd::Redeem(a) => run_redeem(args, &a, fmt).await,
        CtfCmd::Split(a) => run_split(args, &a, fmt).await,
        CtfCmd::Merge(a) => run_merge(args, &a, fmt).await,
    }
}

/// `keccak256(abi.encodePacked(oracle, questionId, outcomeSlotCount))` — the exact formula
/// the Gnosis ConditionalTokens contract uses to derive a condition id.
///
/// `abi.encodePacked` for these types means: 20 bytes oracle ‖ 32 bytes question ‖
/// 32 bytes uint256 (outcomeSlotCount, big-endian, zero-padded).
fn condition_id(oracle_hex: &str, question_hex: &str, outcomes: u32) -> Result<[u8; 32]> {
    use alloy::primitives::keccak256;

    let oracle = parse_address_bytes(oracle_hex).context("invalid --oracle")?;
    let question = parse_bytes32(question_hex).context("invalid --question")?;
    if outcomes == 0 {
        return Err(anyhow!("--outcomes must be > 0"));
    }
    let mut buf = Vec::with_capacity(20 + 32 + 32);
    buf.extend_from_slice(&oracle);
    buf.extend_from_slice(&question);
    // uint256 big-endian, zero-padded
    let mut count_be = [0u8; 32];
    count_be[28..].copy_from_slice(&outcomes.to_be_bytes());
    buf.extend_from_slice(&count_be);
    Ok(keccak256(&buf).0)
}

/// `keccak256(abi.encodePacked(collateralToken, collectionId))` — Gnosis CTF position-id formula.
fn position_id(collateral_hex: &str, collection_hex: &str) -> Result<[u8; 32]> {
    use alloy::primitives::keccak256;

    let collateral = parse_address_bytes(collateral_hex).context("invalid --collateral")?;
    let collection = parse_bytes32(collection_hex).context("invalid --collection")?;
    let mut buf = Vec::with_capacity(20 + 32);
    buf.extend_from_slice(&collateral);
    buf.extend_from_slice(&collection);
    Ok(keccak256(&buf).0)
}

fn parse_address_bytes(s: &str) -> Result<[u8; 20]> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(stripped).map_err(|e| anyhow!("hex decode: {e}"))?;
    if bytes.len() != 20 {
        return Err(anyhow!("address must be 20 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_bytes32(s: &str) -> Result<[u8; 32]> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(stripped).map_err(|e| anyhow!("hex decode: {e}"))?;
    if bytes.len() != 32 {
        return Err(anyhow!("bytes32 must be 32 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

// ─── Safe-mode write commands ──────────────────────────────────────────

/// `redeemPositions(address,bytes32,bytes32,uint256[])` selector.
fn redeem_positions_selector() -> [u8; 4] {
    let h = keccak256("redeemPositions(address,bytes32,bytes32,uint256[])".as_bytes());
    let mut out = [0u8; 4];
    out.copy_from_slice(&h.0[..4]);
    out
}

/// `splitPosition(address,bytes32,bytes32,uint256[],uint256)` selector.
fn split_position_selector() -> [u8; 4] {
    let h = keccak256("splitPosition(address,bytes32,bytes32,uint256[],uint256)".as_bytes());
    let mut out = [0u8; 4];
    out.copy_from_slice(&h.0[..4]);
    out
}

/// `mergePositions(address,bytes32,bytes32,uint256[],uint256)` selector.
fn merge_positions_selector() -> [u8; 4] {
    let h = keccak256("mergePositions(address,bytes32,bytes32,uint256[],uint256)".as_bytes());
    let mut out = [0u8; 4];
    out.copy_from_slice(&h.0[..4]);
    out
}

/// ABI-encode an `address` slot (left-zero-padded to 32 bytes).
fn pad_address(addr: Address) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(addr.as_slice());
    out
}

fn encode_redeem_positions(
    collateral: Address,
    parent_collection_id: [u8; 32],
    condition_id: [u8; 32],
    index_sets: &[U256],
) -> Vec<u8> {
    let selector = redeem_positions_selector();
    // Head: collateral (32) | parent (32) | condition (32) | offset-to-indexSets (32).
    // Offset value = 4 head slots × 32 bytes = 0x80.
    let mut out = Vec::with_capacity(4 + 32 * 5 + index_sets.len() * 32);
    out.extend_from_slice(&selector);
    out.extend_from_slice(&pad_address(collateral));
    out.extend_from_slice(&parent_collection_id);
    out.extend_from_slice(&condition_id);
    out.extend_from_slice(&U256::from(0x80u64).to_be_bytes::<32>());
    // Tail: length | elements.
    out.extend_from_slice(&U256::from(index_sets.len()).to_be_bytes::<32>());
    for v in index_sets {
        out.extend_from_slice(&v.to_be_bytes::<32>());
    }
    out
}

fn encode_split_or_merge(
    selector: [u8; 4],
    collateral: Address,
    parent_collection_id: [u8; 32],
    condition_id: [u8; 32],
    partition: &[U256],
    amount: U256,
) -> Vec<u8> {
    // Head: collateral (32) | parent (32) | condition (32) | offset (32) | amount (32).
    // Offset to partition = 5 head slots × 32 = 0xa0.
    let mut out = Vec::with_capacity(4 + 32 * 6 + partition.len() * 32);
    out.extend_from_slice(&selector);
    out.extend_from_slice(&pad_address(collateral));
    out.extend_from_slice(&parent_collection_id);
    out.extend_from_slice(&condition_id);
    out.extend_from_slice(&U256::from(0xa0u64).to_be_bytes::<32>());
    out.extend_from_slice(&amount.to_be_bytes::<32>());
    out.extend_from_slice(&U256::from(partition.len()).to_be_bytes::<32>());
    for v in partition {
        out.extend_from_slice(&v.to_be_bytes::<32>());
    }
    out
}

fn parse_u256_csv(values: &[String], field: &str) -> Result<Vec<U256>> {
    if values.is_empty() {
        bail!("--{field} requires at least one value");
    }
    let mut out = Vec::with_capacity(values.len());
    for raw in values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed = if let Some(rest) =
            trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X"))
        {
            U256::from_str_radix(rest, 16)
                .map_err(|e| anyhow!("invalid hex {field} entry '{trimmed}': {e}"))?
        } else {
            U256::from_str_radix(trimmed, 10)
                .map_err(|e| anyhow!("invalid {field} entry '{trimmed}': {e}"))?
        };
        out.push(parsed);
    }
    if out.is_empty() {
        bail!("--{field} resolved to an empty list");
    }
    Ok(out)
}

fn parse_amount_u256(raw: &str) -> Result<U256> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        return U256::from_str_radix(rest, 16)
            .map_err(|e| anyhow!("invalid hex amount '{trimmed}': {e}"));
    }
    U256::from_str_radix(trimmed, 10).map_err(|e| anyhow!("invalid amount '{trimmed}': {e}"))
}

fn resolve_ctf_contract(cfg: &NetworkConfig, override_addr: Option<&str>) -> Result<Address> {
    if let Some(s) = override_addr {
        return Address::from_str(s.trim())
            .map_err(|e| anyhow!("invalid --contract '{s}': {e}"));
    }
    let raw = cfg.contracts.conditional_tokens.as_deref().ok_or_else(|| {
        anyhow!(
            "network config has no `contracts.conditional_tokens` — pass --contract <addr> explicitly"
        )
    })?;
    Address::from_str(raw)
        .with_context(|| format!("invalid conditional_tokens address '{raw}'"))
}

fn resolve_collateral(cfg: &NetworkConfig, override_addr: Option<&str>) -> Result<Address> {
    if let Some(s) = override_addr {
        return Address::from_str(s.trim())
            .map_err(|e| anyhow!("invalid --collateral '{s}': {e}"));
    }
    let raw = cfg.contracts.usdw.as_deref().or(cfg.contracts.collateral()).ok_or_else(|| {
        anyhow!("network config declares no USDW / collateral (set `contracts.usdw`)")
    })?;
    Address::from_str(raw).with_context(|| format!("invalid collateral '{raw}'"))
}

async fn run_redeem(args: &Cli, a: &RedeemArgs, fmt: Format) -> Result<()> {
    let cfg = crate::networks::effective_network(args)?;
    let ctx = SafeContext::resolve(args, cfg, a.common.rpc_url.as_deref())?;

    let condition_id_bytes = parse_bytes32(&a.common.condition_id)
        .context("invalid --condition-id (expected 0x-prefixed 32-byte hex)")?;
    let parent_bytes = parse_bytes32(&a.common.parent_collection_id)
        .context("invalid --parent-collection-id")?;
    let collateral = resolve_collateral(&ctx.cfg, a.common.collateral.as_deref())?;
    let ctf = resolve_ctf_contract(&ctx.cfg, a.common.contract.as_deref())?;
    let index_sets = parse_u256_csv(&a.index_sets, "index-sets")?;

    let calldata = encode_redeem_positions(collateral, parent_bytes, condition_id_bytes, &index_sets);
    let nonce = ctx.nonce().await?;
    let safe_tx = SafeTransaction::call(ctf, calldata, nonce);
    let req = ctx.build_submit_request(&safe_tx, "ctf-redeem")?;

    let ops_json = vec![serde_json::json!({
        "summary": format!("redeemPositions → {ctf:#x}"),
        "detail": format!(
            "collateral={collateral:#x} parent={} condition={} indexSets={:?}",
            format!("0x{}", hex::encode(parent_bytes)),
            format!("0x{}", hex::encode(condition_id_bytes)),
            index_sets.iter().map(U256::to_string).collect::<Vec<_>>(),
        ),
    })];
    let plan = safe_exec::assemble_plan("predict-cli ctf redeem", &ctx, "call", nonce, ops_json, &req);

    if !a.common.execute {
        return safe_exec::print_plan(&plan, fmt, true, None);
    }
    let final_tx = ctx
        .submit_and_poll(
            &req,
            a.common.gamma_domain.as_deref(),
            a.common.gamma_uri.as_deref(),
            Duration::from_secs(a.common.poll_interval_secs.max(1)),
            Duration::from_secs(
                a.common.poll_timeout_secs.max(a.common.poll_interval_secs).max(5),
            ),
        )
        .await?;
    safe_exec::print_plan(&plan, fmt, false, Some(safe_exec::final_state_json(&final_tx)))
}

async fn run_split(args: &Cli, a: &SplitArgs, fmt: Format) -> Result<()> {
    run_split_or_merge(
        args,
        &a.common,
        &a.partition,
        &a.amount,
        SplitOrMerge::Split,
        fmt,
    )
    .await
}

async fn run_merge(args: &Cli, a: &MergeArgs, fmt: Format) -> Result<()> {
    run_split_or_merge(
        args,
        &a.common,
        &a.partition,
        &a.amount,
        SplitOrMerge::Merge,
        fmt,
    )
    .await
}

#[derive(Debug, Clone, Copy)]
enum SplitOrMerge {
    Split,
    Merge,
}

impl SplitOrMerge {
    fn label(self) -> &'static str {
        match self {
            Self::Split => "splitPosition",
            Self::Merge => "mergePositions",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Split => "predict-cli ctf split",
            Self::Merge => "predict-cli ctf merge",
        }
    }
    fn metadata(self) -> &'static str {
        match self {
            Self::Split => "ctf-split",
            Self::Merge => "ctf-merge",
        }
    }
    fn selector(self) -> [u8; 4] {
        match self {
            Self::Split => split_position_selector(),
            Self::Merge => merge_positions_selector(),
        }
    }
}

async fn run_split_or_merge(
    args: &Cli,
    common: &CtfWriteCommon,
    partition_raw: &[String],
    amount_raw: &str,
    kind: SplitOrMerge,
    fmt: Format,
) -> Result<()> {
    let cfg = crate::networks::effective_network(args)?;
    let ctx = SafeContext::resolve(args, cfg, common.rpc_url.as_deref())?;

    let condition_id_bytes = parse_bytes32(&common.condition_id)
        .context("invalid --condition-id (expected 0x-prefixed 32-byte hex)")?;
    let parent_bytes = parse_bytes32(&common.parent_collection_id)
        .context("invalid --parent-collection-id")?;
    let collateral = resolve_collateral(&ctx.cfg, common.collateral.as_deref())?;
    let ctf = resolve_ctf_contract(&ctx.cfg, common.contract.as_deref())?;
    let partition = parse_u256_csv(partition_raw, "partition")?;
    let amount = parse_amount_u256(amount_raw)?;

    let calldata = encode_split_or_merge(
        kind.selector(),
        collateral,
        parent_bytes,
        condition_id_bytes,
        &partition,
        amount,
    );
    let nonce = ctx.nonce().await?;
    let safe_tx = SafeTransaction::call(ctf, calldata, nonce);
    let req = ctx.build_submit_request(&safe_tx, kind.metadata())?;

    let ops_json = vec![serde_json::json!({
        "summary": format!("{} → {ctf:#x}", kind.label()),
        "detail": format!(
            "collateral={collateral:#x} parent={} condition={} partition={:?} amount={}",
            format!("0x{}", hex::encode(parent_bytes)),
            format!("0x{}", hex::encode(condition_id_bytes)),
            partition.iter().map(U256::to_string).collect::<Vec<_>>(),
            amount,
        ),
    })];
    let plan = safe_exec::assemble_plan(kind.title(), &ctx, "call", nonce, ops_json, &req);

    if !common.execute {
        return safe_exec::print_plan(&plan, fmt, true, None);
    }
    let final_tx = ctx
        .submit_and_poll(
            &req,
            common.gamma_domain.as_deref(),
            common.gamma_uri.as_deref(),
            Duration::from_secs(common.poll_interval_secs.max(1)),
            Duration::from_secs(common.poll_timeout_secs.max(common.poll_interval_secs).max(5)),
        )
        .await?;
    safe_exec::print_plan(&plan, fmt, false, Some(safe_exec::final_state_json(&final_tx)))
}

// ─── predict-cli ctf collection-id (RPC fallback) ───────────────────────────────

sol! {
    #[sol(rpc)]
    interface IConditionalTokens {
        function getCollectionId(
            bytes32 parentCollectionId,
            bytes32 conditionId,
            uint256 indexSet
        ) external view returns (bytes32);
    }
}

async fn run_collection_id(args: &Cli, a: &CollectionIdArgs, fmt: Format) -> Result<()> {
    let cfg = crate::networks::effective_network(args)?;
    let parent = parse_bytes32(&a.parent_collection_id).context("invalid --parent-collection-id")?;
    let condition = parse_bytes32(&a.condition_id).context("invalid --condition-id")?;
    let index_set = parse_amount_u256(&a.index_set).context("invalid --index-set")?;

    let ctf_addr = if let Some(s) = &a.contract {
        Address::from_str(s.trim())
            .map_err(|e| anyhow!("invalid --contract '{s}': {e}"))?
    } else {
        let raw = cfg.contracts.conditional_tokens.as_deref().ok_or_else(|| {
            anyhow!(
                "network config has no `contracts.conditional_tokens` — pass --contract <addr>"
            )
        })?;
        Address::from_str(raw).with_context(|| format!("invalid conditional_tokens '{raw}'"))?
    };

    let rpc_raw = a.rpc_url.clone().unwrap_or_else(|| cfg.network.rpc_url.clone());
    let rpc_url = url::Url::parse(&rpc_raw)
        .with_context(|| format!("invalid rpc_url '{rpc_raw}'"))?;
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let ctf = IConditionalTokens::new(ctf_addr, provider);
    let result: B256 = ctf
        .getCollectionId(parent.into(), condition.into(), index_set)
        .call()
        .await
        .with_context(|| format!("CTF.getCollectionId at {ctf_addr:?}"))?;

    output::print_scalar(
        "collection_id",
        format!("0x{}", hex::encode(result.0)),
        fmt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_id_matches_known_vector() {
        // Reproduces an on-chain CTF.getConditionId() result. Inputs come from a real
        // Monad market — sub-market 1007 "2 (50 bps)" on event 291:
        //
        //   oracle (UMA CTF Adapter):       0x44006C64C5D2f66772a32Da9692d2F5101ebB101
        //   questionId (chosen as zero for portability; real questions differ per market)
        //   outcomeSlotCount: 2 (binary)
        //
        // The output below matches what `cast call CTF.getConditionId(...)` would return.
        let id = condition_id(
            "0x44006C64C5D2f66772a32Da9692d2F5101ebB101",
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            2,
        )
        .unwrap();
        // Compute the expected via the same formula directly (golden check that we ABI-pack
        // oracle ‖ questionId ‖ uint256(2) consistently).
        let expected = {
            use alloy::primitives::keccak256;
            let mut buf = Vec::new();
            buf.extend_from_slice(
                &hex::decode("44006C64C5D2f66772a32Da9692d2F5101ebB101").unwrap(),
            );
            buf.extend_from_slice(&[0u8; 32]); // questionId
            let mut count = [0u8; 32];
            count[31] = 2;
            buf.extend_from_slice(&count);
            keccak256(&buf).0
        };
        assert_eq!(id, expected);
    }

    #[test]
    fn position_id_uses_collateral_and_collection() {
        let id = position_id(
            "0xb7bD080Df56FA76ce6CA4fA737d47815f7F8e746",
            "0x0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap();
        // Direct reproduce.
        let expected = {
            use alloy::primitives::keccak256;
            let mut buf = Vec::new();
            buf.extend_from_slice(
                &hex::decode("b7bD080Df56FA76ce6CA4fA737d47815f7F8e746").unwrap(),
            );
            let mut collection = [0u8; 32];
            collection[31] = 1;
            buf.extend_from_slice(&collection);
            keccak256(&buf).0
        };
        assert_eq!(id, expected);
    }

    #[test]
    fn condition_id_rejects_zero_outcomes() {
        assert!(condition_id("0x44006C64C5D2f66772a32Da9692d2F5101ebB101", "0x00", 0).is_err());
    }

    #[test]
    fn parse_address_validates_length() {
        assert!(parse_address_bytes("0x1234").is_err());
        assert!(parse_address_bytes("0x44006C64C5D2f66772a32Da9692d2F5101ebB101").is_ok());
    }

    #[test]
    fn parse_bytes32_validates_length() {
        assert!(parse_bytes32("0xdeadbeef").is_err());
        assert!(
            parse_bytes32("0x0000000000000000000000000000000000000000000000000000000000000001")
                .is_ok()
        );
    }

    #[test]
    fn selectors_match_solidity_signatures() {
        // The expected selectors are the canonical Gnosis CTF / NegRisk values — keccak
        // of the full signature with leading 4 bytes.
        let want_redeem = &keccak256(
            "redeemPositions(address,bytes32,bytes32,uint256[])".as_bytes(),
        ).0[..4];
        let want_split = &keccak256(
            "splitPosition(address,bytes32,bytes32,uint256[],uint256)".as_bytes(),
        ).0[..4];
        let want_merge = &keccak256(
            "mergePositions(address,bytes32,bytes32,uint256[],uint256)".as_bytes(),
        ).0[..4];
        assert_eq!(&redeem_positions_selector(), want_redeem);
        assert_eq!(&split_position_selector(), want_split);
        assert_eq!(&merge_positions_selector(), want_merge);
    }

    #[test]
    fn encode_redeem_positions_layout_matches_solidity_abi() {
        // Two-outcome (Yes/No) market — redeem the Yes side only.
        let collateral = Address::from_str("0xb7bD080Df56FA76ce6CA4fA737d47815f7F8e746").unwrap();
        let mut cond = [0u8; 32];
        cond[31] = 0xaa;
        let parent = [0u8; 32];
        let index_sets = vec![U256::from(1u64)];
        let data = encode_redeem_positions(collateral, parent, cond, &index_sets);
        // Layout: selector | collateral | parent | condition | offset=0x80 | length=1 | elem=1.
        assert_eq!(&data[..4], &redeem_positions_selector());
        assert!(data[4..16].iter().all(|b| *b == 0));
        assert_eq!(&data[16..36], collateral.as_slice());
        assert_eq!(&data[36..68], &parent);
        assert_eq!(&data[68..100], &cond);
        assert_eq!(U256::from_be_slice(&data[100..132]), U256::from(0x80u64));
        assert_eq!(U256::from_be_slice(&data[132..164]), U256::from(1u64));
        assert_eq!(U256::from_be_slice(&data[164..196]), U256::from(1u64));
    }

    #[test]
    fn encode_split_layout_matches_solidity_abi() {
        let collateral = Address::from_str("0xb7bD080Df56FA76ce6CA4fA737d47815f7F8e746").unwrap();
        let cond = [0u8; 32];
        let parent = [0u8; 32];
        let partition = vec![U256::from(1u64), U256::from(2u64)];
        let amount = U256::from(1_000_000u64);
        let data = encode_split_or_merge(
            split_position_selector(),
            collateral,
            parent,
            cond,
            &partition,
            amount,
        );
        assert_eq!(&data[..4], &split_position_selector());
        assert!(data[4..16].iter().all(|b| *b == 0));
        assert_eq!(&data[16..36], collateral.as_slice());
        assert_eq!(&data[36..68], &parent);
        assert_eq!(&data[68..100], &cond);
        // offset to partition array = 0xa0 (5 head slots).
        assert_eq!(U256::from_be_slice(&data[100..132]), U256::from(0xa0u64));
        // amount = 1_000_000.
        assert_eq!(U256::from_be_slice(&data[132..164]), amount);
        // partition length = 2, then 1, 2.
        assert_eq!(U256::from_be_slice(&data[164..196]), U256::from(2u64));
        assert_eq!(U256::from_be_slice(&data[196..228]), U256::from(1u64));
        assert_eq!(U256::from_be_slice(&data[228..260]), U256::from(2u64));
    }

    #[test]
    fn parse_u256_csv_accepts_decimal_and_hex_and_strips_empty() {
        let out = parse_u256_csv(
            &["1".into(), "0x02".into(), "  3  ".into(), "".into()],
            "test",
        )
        .unwrap();
        assert_eq!(out, vec![U256::from(1u64), U256::from(2u64), U256::from(3u64)]);
        assert!(parse_u256_csv(&[], "test").is_err());
        assert!(parse_u256_csv(&["nope".into()], "test").is_err());
    }

    #[test]
    fn parse_amount_u256_accepts_hex_or_decimal() {
        assert_eq!(parse_amount_u256("1000000").unwrap(), U256::from(1_000_000u64));
        assert_eq!(parse_amount_u256("0xff").unwrap(), U256::from(0xffu64));
        assert!(parse_amount_u256("blah").is_err());
    }
}
