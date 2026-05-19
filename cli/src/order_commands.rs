//! `pm order …` and `pm trade …` subcommand wiring (Phase 2.2).
//!
//! Maps the user-facing clap structs to the SDK's [`pm_rs_clob_client::OrderBuilder`] +
//! `Client::post_order` / `Client::cancel_*` / `Client::open_orders` / `Client::trades`
//! surface. Every flag is optional except the ones that affect signing (token / side /
//! price-or-amount / fee-rate / maker for Safe).

use anyhow::{Context, anyhow};
use clap::{Args, Subcommand, ValueEnum};
use rust_decimal::Decimal;
use serde_json::json;
use std::str::FromStr;

use pm_rs_clob_client::clob::order_builder::{Limit, Market, OrderBuilder};
use pm_rs_clob_client::clob::types::{
    CancelMarketOrderRequest, OpenOrderResponse, OrderType, OrdersRequest, Page,
    PostOrderResponse, ReplaceOrdersRequest, SendOrderRequest, SignableOrder, SignedOrder,
    TradeResponse, TradesRequest,
};
use pm_rs_clob_client::types::{Address, SignatureType, U256};
use pm_rs_clob_client::{PMCup26Signer, Side};

use crate::cli::{Cli, SideArg, SignatureTypeArg};
use crate::commands::{parse_address, signer_from_args, with_l2_credentials};
use crate::output::{self, Format};

/// `pm order <SUBCOMMAND>`.
#[derive(Debug, Subcommand)]
pub enum OrderCommand {
    /// Build, sign, then `POST /order`. Use `--dry-run` to inspect the JSON without posting.
    Create(CreateArgs),
    /// `DELETE /order` by id.
    Cancel(CancelArgs),
    /// `DELETE /orders` — batch cancel up to 3000 ids (comma-separated).
    CancelMany(CancelManyArgs),
    /// `DELETE /cancel-market-orders` — by condition id and/or token id.
    CancelMarket(CancelMarketArgs),
    /// `DELETE /cancel-all` — cancel everything for the API key.
    CancelAll,
    /// `GET /orders` — paginated open-order list.
    List(ListArgs),
    /// `GET /order/{ORDER_ID}` — single order detail.
    Get(GetArgs),
    /// `POST /orders/replace` — cancel old + place new in one shot.
    Replace(ReplaceArgs),
    /// `GET /order-scoring` — maker-program eligibility lookup.
    Scoring(ScoringArgs),
}

/// `pm order create` arguments — mirrors `Polymarket clob create-order` shape with chainup
/// renames. `--limit` / `--market` selects the builder variant (limit by default).
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Token id (uint256 decimal). Required.
    #[arg(long)]
    pub token: String,
    /// Order side.
    #[arg(long, value_enum)]
    pub side: SideArg,
    /// Limit price in (0, 1).
    #[arg(long)]
    pub price: Option<Decimal>,
    /// Order size in shares (limit orders) or share-denominated market amount.
    #[arg(long)]
    pub size: Option<Decimal>,
    /// Market-order amount denominated in USDC (BUY only). Mutually exclusive with `--size`.
    #[arg(long, conflicts_with = "size")]
    pub amount: Option<Decimal>,
    /// Force a market order (FAK by default).
    #[arg(long, conflicts_with = "limit")]
    pub market: bool,
    /// Force a limit order (GTC by default). Default when neither `--market` nor `--limit`
    /// is set.
    #[arg(long)]
    pub limit: bool,
    /// Override the default order type (`GTC` for limit, `FAK` for market).
    #[arg(long, value_enum)]
    pub order_type: Option<OrderTypeArg>,
    /// `postOnly` flag — limit orders only.
    #[arg(long)]
    pub post_only: bool,
    /// Unix-seconds expiration. Required when `--order-type GTD`.
    #[arg(long)]
    pub expiration: Option<u64>,
    /// Server-side rotation nonce (default 0).
    #[arg(long, default_value_t = 0)]
    pub nonce: u64,
    /// Fee rate in basis points — required. The server rejects orders below the event
    /// minimum.
    #[arg(long)]
    pub fee_rate_bps: u64,
    /// Maker (Safe-wallet) address. Required for `signatureType = gnosis-safe` (default).
    #[arg(long)]
    pub maker: Option<String>,
    /// Taker address (default zero = any taker).
    #[arg(long)]
    pub taker: Option<String>,
    /// EIP-712 signature type. Default `gnosis-safe`.
    #[arg(long, value_enum, default_value = "gnosis-safe")]
    pub signature_type: SignatureTypeArg,
    /// Optional `owner` UUID. When empty the server uses the API-key owner.
    #[arg(long)]
    pub owner: Option<String>,
    /// Optional salt (uint256 decimal). When omitted the SDK uses an ns-time seed.
    #[arg(long)]
    pub salt: Option<String>,
    /// Print the signed order JSON and exit — do NOT POST.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OrderTypeArg {
    Gtc,
    Gtd,
    Fok,
    Fak,
}

impl From<OrderTypeArg> for OrderType {
    fn from(v: OrderTypeArg) -> Self {
        match v {
            OrderTypeArg::Gtc => OrderType::Gtc,
            OrderTypeArg::Gtd => OrderType::Gtd,
            OrderTypeArg::Fok => OrderType::Fok,
            OrderTypeArg::Fak => OrderType::Fak,
        }
    }
}

#[derive(Debug, Args)]
pub struct CancelArgs {
    /// Order ID (snowflake) to cancel.
    pub order_id: String,
}

#[derive(Debug, Args)]
pub struct CancelManyArgs {
    /// Comma-separated list of order ids (max 3000).
    pub ids: String,
}

#[derive(Debug, Args)]
pub struct CancelMarketArgs {
    /// Condition ID (`market` parameter).
    #[arg(long)]
    pub market: Option<String>,
    /// Token ID (`asset_id` parameter).
    #[arg(long)]
    pub asset_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub market: Option<String>,
    #[arg(long)]
    pub asset_id: Option<String>,
    /// `live` (default), `all`, or an `OrderStatus` literal.
    #[arg(long)]
    pub status: Option<String>,
    /// Forward-pagination cursor (`next_cursor` from a previous page).
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    pub order_id: String,
}

#[derive(Debug, Args)]
pub struct ReplaceArgs {
    /// Comma-separated list of order ids to cancel.
    #[arg(long, value_delimiter = ',')]
    pub cancel: Vec<String>,
    /// JSON file containing an array of `SendOrder` envelopes — same shape as `POST /orders`.
    /// The CLI does not currently rebuild + sign new orders inline (use `pm order create
    /// --dry-run` to mint the JSON for each).
    #[arg(long)]
    pub orders_file: Option<String>,
}

#[derive(Debug, Args)]
pub struct ScoringArgs {
    pub order_id: String,
}

/// `pm trade list` arguments.
#[derive(Debug, Args)]
pub struct TradeArgs {
    #[arg(long)]
    pub market: Option<String>,
    #[arg(long)]
    pub asset_id: Option<String>,
    /// Unix-seconds upper bound.
    #[arg(long)]
    pub before: Option<i64>,
    /// Unix-seconds lower bound.
    #[arg(long)]
    pub after: Option<i64>,
    /// Snowflake `from_id` ASC cursor (chainup-specific).
    #[arg(long)]
    pub from_id: Option<i64>,
    /// Page size [1, 1000]. Server default 100.
    #[arg(long)]
    pub limit: Option<u32>,
    /// Pagination cursor.
    #[arg(long)]
    pub cursor: Option<String>,
    /// Switch to the Builder-only endpoint (`GET /builder/trades`, limit 300).
    #[arg(long)]
    pub builder: bool,
}

// ─── entry points ────────────────────────────────────────────────────────

pub async fn run(args: &Cli, sub: &OrderCommand, fmt: Format) -> anyhow::Result<()> {
    match sub {
        OrderCommand::Create(a) => run_create(args, a, fmt).await,
        OrderCommand::Cancel(a) => run_cancel(args, a, fmt).await,
        OrderCommand::CancelMany(a) => run_cancel_many(args, a, fmt).await,
        OrderCommand::CancelMarket(a) => run_cancel_market(args, a, fmt).await,
        OrderCommand::CancelAll => run_cancel_all(args, fmt).await,
        OrderCommand::List(a) => run_list(args, a, fmt).await,
        OrderCommand::Get(a) => run_get(args, a, fmt).await,
        OrderCommand::Replace(a) => run_replace(args, a, fmt).await,
        OrderCommand::Scoring(a) => run_scoring(args, a, fmt).await,
    }
}

pub async fn run_trade(args: &Cli, a: &TradeArgs, fmt: Format) -> anyhow::Result<()> {
    let req = TradesRequest {
        maker_address: None, // SDK fills from signer
        id: None,
        market: a.market.clone(),
        asset_id: a.asset_id.clone(),
        before: a.before,
        after: a.after,
        from_id: a.from_id,
        limit: a.limit,
    };
    let cursor = a.cursor.clone();
    let builder = a.builder;
    let page = with_l2_credentials(args, move |c| async move {
        let cur = cursor.as_deref();
        if builder {
            c.builder_trades(&req, cur).await
        } else {
            c.trades(&req, cur).await
        }
    })
    .await?;
    print_trades(&page, fmt)
}

pub async fn run_heartbeat(args: &Cli, fmt: Format) -> anyhow::Result<()> {
    let resp = with_l2_credentials(args, |c| async move { c.heartbeat().await }).await?;
    output::print_scalar("status", &resp.status, fmt)
}

// ─── order subcommands ───────────────────────────────────────────────────

async fn run_create(args: &Cli, a: &CreateArgs, fmt: Format) -> anyhow::Result<()> {
    let signer = signer_from_args(args)?;
    if signer.address() == Address::ZERO {
        // Defensive: signer derivation should always yield a non-zero address.
        return Err(anyhow!("signer derived a zero address"));
    }
    let (signable, signed) = build_signed_order(&signer, a)?;
    if a.dry_run {
        // Print the full SendOrder envelope shape — what the CLI would POST.
        let envelope = SendOrderRequest {
            order: signed,
            owner: signable.owner.clone(),
            order_type: signable.order_type,
            post_only: signable.post_only,
            defer_exec: false,
        };
        output::print_json(&serde_json::to_value(&envelope)?)?;
        return Ok(());
    }
    let order_type = signable.order_type;
    let post_only = signable.post_only;
    let owner = signable.owner.clone();
    let resp = with_l2_credentials(args, move |c| async move {
        c.post_order(signed, order_type, post_only, owner).await
    })
    .await?;
    print_post_order(&resp, fmt)
}

async fn run_cancel(args: &Cli, a: &CancelArgs, fmt: Format) -> anyhow::Result<()> {
    let id = a.order_id.clone();
    let resp = with_l2_credentials(args, move |c| async move { c.cancel_order(&id).await }).await?;
    print_cancel(&resp, fmt)
}

async fn run_cancel_many(args: &Cli, a: &CancelManyArgs, fmt: Format) -> anyhow::Result<()> {
    let ids: Vec<String> = a
        .ids
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let resp = with_l2_credentials(args, move |c| async move { c.cancel_orders(&ids).await }).await?;
    print_cancel(&resp, fmt)
}

async fn run_cancel_market(args: &Cli, a: &CancelMarketArgs, fmt: Format) -> anyhow::Result<()> {
    let req = CancelMarketOrderRequest {
        market: a.market.clone(),
        asset_id: a.asset_id.clone(),
    };
    let resp = with_l2_credentials(args, move |c| async move { c.cancel_market_orders(req).await })
        .await?;
    print_cancel(&resp, fmt)
}

async fn run_cancel_all(args: &Cli, fmt: Format) -> anyhow::Result<()> {
    let resp = with_l2_credentials(args, |c| async move { c.cancel_all().await }).await?;
    print_cancel(&resp, fmt)
}

async fn run_list(args: &Cli, a: &ListArgs, fmt: Format) -> anyhow::Result<()> {
    let req = OrdersRequest {
        id: a.id.clone(),
        market: a.market.clone(),
        asset_id: a.asset_id.clone(),
        status: a.status.clone(),
    };
    let cursor = a.cursor.clone();
    let page = with_l2_credentials(args, move |c| async move {
        c.open_orders(&req, cursor.as_deref()).await
    })
    .await?;
    print_open_orders(&page, fmt)
}

async fn run_get(args: &Cli, a: &GetArgs, fmt: Format) -> anyhow::Result<()> {
    let id = a.order_id.clone();
    let resp =
        with_l2_credentials(args, move |c| async move { c.open_order(&id).await }).await?;
    output::print_json(&serde_json::to_value(serializable_order(&resp))?)?;
    let _ = fmt; // open-order rendering currently always JSON; tabled view would need many cols.
    Ok(())
}

async fn run_replace(args: &Cli, a: &ReplaceArgs, fmt: Format) -> anyhow::Result<()> {
    let orders: Vec<SendOrderRequest> = match &a.orders_file {
        None => Vec::new(),
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("read --orders-file {path}"))?;
            serde_json::from_str::<Vec<SendOrderRequest>>(&raw)
                .with_context(|| format!("decode --orders-file {path}"))?
        }
    };
    let req = ReplaceOrdersRequest {
        cancel_order_ids: a.cancel.clone(),
        orders,
    };
    let resp = with_l2_credentials(args, move |c| async move { c.replace_order(req).await }).await?;
    output::print_json(&json!({
        "stoppedAt": resp.stopped_at,
        "errorMsg": resp.error_msg,
        "cancels": resp.cancels.into_iter().map(|c| json!({"orderID": c.order_id, "status": c.status})).collect::<Vec<_>>(),
        "placements": resp.placements.into_iter().map(|p| json!({
            "index": p.index,
            "success": p.success,
            "errorMsg": p.error_msg,
            "orderID": p.order_id,
            "status": p.status,
            "takingAmount": p.taking_amount,
            "makingAmount": p.making_amount,
            "tradeIDs": p.trade_ids,
            "transactionsHashes": p.transactions_hashes,
        })).collect::<Vec<_>>(),
    }))?;
    let _ = fmt;
    Ok(())
}

async fn run_scoring(args: &Cli, a: &ScoringArgs, fmt: Format) -> anyhow::Result<()> {
    let id = a.order_id.clone();
    let resp =
        with_l2_credentials(args, move |c| async move { c.order_scoring(&id).await }).await?;
    output::print_scalar("scoring", resp.scoring, fmt)
}

// ─── helpers ─────────────────────────────────────────────────────────────

fn build_signed_order(
    signer: &PMCup26Signer,
    a: &CreateArgs,
) -> anyhow::Result<(SignableOrder, SignedOrder)> {
    let token_id = U256::from_str(&a.token)
        .with_context(|| format!("invalid --token (must be uint256 decimal): {}", a.token))?;
    let side: Side = a.side.into();
    let signature_type: SignatureType = a.signature_type.into();

    let maker = match &a.maker {
        Some(s) => parse_address(s).with_context(|| format!("invalid --maker {s}"))?,
        None if signature_type == SignatureType::PolyGnosisSafe => {
            return Err(anyhow!(
                "--maker is required for signature_type=gnosis-safe (chainup default). \
                 Pass --maker <Safe address> or --signature-type eoa."
            ));
        }
        None => signer.address(),
    };
    let taker = match &a.taker {
        Some(s) => parse_address(s).with_context(|| format!("invalid --taker {s}"))?,
        None => Address::ZERO,
    };

    // Pick variant + finalise.
    let order_type_default = if a.market {
        OrderType::Fak
    } else {
        OrderType::Gtc
    };
    let order_type = a.order_type.map(Into::into).unwrap_or(order_type_default);

    let salt_override = match &a.salt {
        Some(s) => Some(U256::from_str(s).with_context(|| format!("invalid --salt {s}"))?),
        None => None,
    };

    if a.market {
        let mut b: OrderBuilder<Market> = OrderBuilder::<Market>::market()
            .token_id(token_id)
            .side(side)
            .order_type(order_type)
            .fee_rate_bps(a.fee_rate_bps)
            .maker(maker)
            .taker(taker)
            .signature_type(signature_type)
            .post_only(a.post_only)
            .nonce(a.nonce)
            .expiration(a.expiration.unwrap_or(0));
        if let Some(p) = a.price {
            b = b.price(p);
        }
        if let Some(s) = a.size {
            b = b.shares(s);
        } else if let Some(u) = a.amount {
            b = b.usdc(u);
        }
        if let Some(owner) = &a.owner {
            b = b.owner(owner.clone());
        }
        if let Some(salt) = salt_override {
            b = b.salt(salt);
        }
        Ok(b.build_and_sign(signer)?)
    } else {
        let mut b: OrderBuilder<Limit> = OrderBuilder::<Limit>::limit()
            .token_id(token_id)
            .side(side)
            .order_type(order_type)
            .fee_rate_bps(a.fee_rate_bps)
            .maker(maker)
            .taker(taker)
            .signature_type(signature_type)
            .post_only(a.post_only)
            .nonce(a.nonce)
            .expiration(a.expiration.unwrap_or(0));
        if let Some(p) = a.price {
            b = b.price(p);
        }
        if let Some(s) = a.size {
            b = b.size(s);
        }
        if let Some(owner) = &a.owner {
            b = b.owner(owner.clone());
        }
        if let Some(salt) = salt_override {
            b = b.salt(salt);
        }
        Ok(b.build_and_sign(signer)?)
    }
}

fn serializable_order(o: &OpenOrderResponse) -> serde_json::Value {
    json!({
        "id": o.id,
        "status": o.status,
        "owner": o.owner,
        "maker_address": o.maker_address,
        "market": o.market,
        "asset_id": o.asset_id,
        "side": o.side,
        "outcome": o.outcome,
        "original_size": o.original_size,
        "size_matched": o.size_matched,
        "price": o.price,
        "expiration": o.expiration,
        "order_type": o.order_type,
        "created_at": o.created_at,
        "associate_trades": o.associate_trades,
        "lazy": o.lazy,
    })
}

fn print_post_order(resp: &PostOrderResponse, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&json!({
            "success": resp.success,
            "errorMsg": resp.error_msg,
            "orderID": resp.order_id,
            "status": resp.status,
            "takingAmount": resp.taking_amount,
            "makingAmount": resp.making_amount,
            "transactionsHashes": resp.transactions_hashes,
            "tradeIDs": resp.trade_ids,
        }))?,
        Format::Table => {
            println!("success       : {}", resp.success);
            if !resp.error_msg.is_empty() {
                println!("errorMsg      : {}", resp.error_msg);
            }
            println!("orderID       : {}", resp.order_id);
            println!("status        : {}", resp.status);
            println!("takingAmount  : {}", resp.taking_amount);
            println!("makingAmount  : {}", resp.making_amount);
            if !resp.transactions_hashes.is_empty() {
                println!("tx_hashes     : {}", resp.transactions_hashes.join(", "));
            }
            if !resp.trade_ids.is_empty() {
                println!("tradeIDs      : {}", resp.trade_ids.join(", "));
            }
        }
    }
    Ok(())
}

fn print_cancel(
    resp: &pm_rs_clob_client::CancelOrdersResponse,
    fmt: Format,
) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&json!({
            "canceled": resp.canceled,
            "not_canceled": resp.not_canceled,
        }))?,
        Format::Table => {
            if resp.canceled.is_empty() {
                println!("canceled  : (none)");
            } else {
                println!("canceled  : {}", resp.canceled.join(", "));
            }
            if !resp.not_canceled.is_empty() {
                for (id, reason) in &resp.not_canceled {
                    println!("  not_canceled[{id}]: {reason}");
                }
            }
        }
    }
    Ok(())
}

fn print_open_orders(page: &Page<OpenOrderResponse>, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&json!({
            "limit": page.limit,
            "count": page.count,
            "next_cursor": page.next_cursor,
            "data": page.data.iter().map(serializable_order).collect::<Vec<_>>(),
        }))?,
        Format::Table => {
            if page.data.is_empty() {
                println!("(no orders)");
            } else {
                println!(
                    "{:<22} {:<6} {:<20} {:<12} {:<8} {:<8} {:<8}",
                    "id", "side", "market", "asset_id", "price", "size", "matched"
                );
                for o in &page.data {
                    println!(
                        "{:<22} {:<6} {:<20} {:<12} {:<8} {:<8} {:<8}",
                        truncate(&o.id, 22),
                        o.side,
                        truncate(&o.market, 20),
                        truncate(&o.asset_id, 12),
                        o.price,
                        o.original_size,
                        o.size_matched
                    );
                }
            }
            println!("limit={} count={} next_cursor={}", page.limit, page.count, page.next_cursor);
        }
    }
    Ok(())
}

fn print_trades(page: &Page<TradeResponse>, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&json!({
            "limit": page.limit,
            "count": page.count,
            "next_cursor": page.next_cursor,
            "data": page.data.iter().map(|t| json!({
                "id": t.id,
                "taker_order_id": t.taker_order_id,
                "market": t.market,
                "asset_id": t.asset_id,
                "side": t.side,
                "size": t.size,
                "price": t.price,
                "fee_rate_bps": t.fee_rate_bps,
                "fee": t.fee,
                "status": t.status,
                "match_time": t.match_time,
                "match_type": t.match_type,
                "order_type": t.order_type,
                "transaction_hash": t.transaction_hash,
                "trader_side": t.trader_side,
            })).collect::<Vec<_>>(),
        }))?,
        Format::Table => {
            if page.data.is_empty() {
                println!("(no trades)");
            } else {
                println!(
                    "{:<22} {:<6} {:<12} {:<8} {:<8} {:<22} {:<10}",
                    "id", "side", "asset_id", "price", "size", "match_time", "status"
                );
                for t in &page.data {
                    println!(
                        "{:<22} {:<6} {:<12} {:<8} {:<8} {:<22} {:<10}",
                        truncate(&t.id, 22),
                        t.side,
                        truncate(&t.asset_id, 12),
                        t.price,
                        t.size,
                        truncate(&t.match_time, 22),
                        truncate(&t.status, 10),
                    );
                }
            }
            println!("limit={} count={} next_cursor={}", page.limit, page.count, page.next_cursor);
        }
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        s.to_owned()
    }
}
