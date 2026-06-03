//! CLI subcommands for the `data-service`.
//!
//! Mirrors the upstream CLI's `data {positions / closed-positions / trades / activity /
//! holders / open-interest / live-volume / leaderboard / ...}` tree, dropping commands
//! whose endpoint doesn't exist here (`builder-leaderboard`, `builder-volume`,
//! `value`) and adding platform-only ones (`unwrap-requests`).
//!
//! Wired into [`crate::commands::run`] via a `Command::Data` arm so the diff with shared
//! CLI files stays minimal. Endpoints are public read-only (no L1 / L2 auth required).

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use predict_rs_clob_client::Client;

use crate::output::{self, Format};

#[derive(Debug, Args)]
pub struct DataArgs {
    #[command(subcommand)]
    pub command: DataCmd,
}

#[derive(Debug, Subcommand)]
pub enum DataCmd {
    /// `GET /positions` — open positions for a wallet.
    Positions(WalletPaginateArgs),
    /// `GET /closed-positions` — closed positions for a wallet.
    ClosedPositions(WalletPaginateArgs),
    /// `GET /v1/market-positions` — position leaderboard for a single market.
    MarketPositions(MarketPaginateArgs),
    /// `GET /trades` — trade history for a wallet.
    Trades(WalletPaginateArgs),
    /// `GET /activity` — on-chain activity for a wallet (trades + splits + merges + redeems).
    Activity(WalletPaginateArgs),
    /// `GET /holders` — top token holders for a market.
    Holders(HoldersArgs),
    /// `GET /traded` — count of unique markets traded by a wallet.
    Traded(WalletArg),
    /// `GET /oi` — open interest for one market.
    OpenInterest(MarketArg),
    /// `GET /live-volume` — live volume for an event.
    LiveVolume(EventArg),
    /// `GET /prices-history` — token mid-price history.
    PricesHistory(PricesHistoryArgs),
    /// `GET /user-pnl` — cumulative profit/loss time-series for a wallet.
    UserPnl(UserPnlArgs),
    /// `GET /stats` — global platform statistics.
    Stats,
    /// `GET /v1/leaderboard` — trader leaderboard (with biggest-wins sidebar).
    Leaderboard(LeaderboardArgs),
    /// `GET /unwrap-requests` — USDW unwrap queue for a Safe address.
    UnwrapRequests(UnwrapRequestsArgs),
}

#[derive(Debug, Args)]
pub struct WalletArg {
    /// Wallet address (0x...).
    pub address: String,
}

#[derive(Debug, Args)]
pub struct WalletPaginateArgs {
    /// Wallet address (0x...).
    pub address: String,
    /// Max results.
    #[arg(long, default_value_t = 25)]
    pub limit: i32,
    /// Pagination offset.
    #[arg(long)]
    pub offset: Option<i32>,
}

#[derive(Debug, Args)]
pub struct MarketArg {
    /// Market condition id (0x...).
    pub market: String,
}

#[derive(Debug, Args)]
pub struct EventArg {
    /// Event id.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct MarketPaginateArgs {
    /// Market condition id (0x...).
    pub condition_id: String,
    #[arg(long, default_value_t = 25)]
    pub limit: i32,
    #[arg(long)]
    pub offset: Option<i32>,
}

#[derive(Debug, Args)]
pub struct HoldersArgs {
    /// Market condition id (0x...).
    pub market: String,
    /// Max results per token.
    #[arg(long)]
    pub limit: Option<i32>,
}

#[derive(Debug, Args)]
pub struct PricesHistoryArgs {
    /// Token id (or market id, depending on data-service implementation).
    pub market: String,
    /// Bucket interval (`1m / 1h / 6h / 1d / max`).
    #[arg(long)]
    pub interval: Option<String>,
    /// Fidelity in seconds.
    #[arg(long)]
    pub fidelity: Option<i64>,
}

#[derive(Debug, Args)]
pub struct UserPnlArgs {
    /// Wallet address (0x...).
    pub address: String,
    /// Window (`1d / 1w / 1m / all`).
    #[arg(long)]
    pub interval: Option<String>,
    /// Bucket size (`1h / 3h / 12h / 18h / 1d`).
    #[arg(long)]
    pub fidelity: Option<String>,
}

#[derive(Debug, Args)]
pub struct LeaderboardArgs {
    /// Time window (`DAY / WEEK / MONTH / ALL`, default DAY).
    #[arg(long)]
    pub time_period: Option<String>,
    /// Sort key (`PNL / VOL`, default PNL).
    #[arg(long)]
    pub order_by: Option<String>,
    #[arg(long)]
    pub limit: Option<i32>,
    #[arg(long)]
    pub offset: Option<i32>,
}

#[derive(Debug, Args)]
pub struct UnwrapRequestsArgs {
    /// Safe address that initiated `initiateUnwrap` (recipient of the wrapped balance).
    pub safe: String,
    /// `false` (default) → only pending requests; `true` → only claimed-history.
    #[arg(long)]
    pub claimed: Option<bool>,
}

pub async fn run(client: Client, fmt: Format, args: DataArgs) -> Result<()> {
    let data = client.data().context("data-service endpoint not configured")?;

    match args.command {
        DataCmd::Positions(a) => {
            let rows = data.positions(&a.address, a.limit, a.offset).await?;
            output::print_json(&rows)?;
        }
        DataCmd::ClosedPositions(a) => {
            let rows = data
                .closed_positions(&a.address, a.limit, a.offset)
                .await?;
            output::print_json(&rows)?;
        }
        DataCmd::MarketPositions(a) => {
            let rows = data
                .market_positions(&a.condition_id, a.limit, a.offset)
                .await?;
            output::print_json(&rows)?;
        }
        DataCmd::Trades(a) => {
            let rows = data.trades(&a.address, a.limit, a.offset).await?;
            output::print_json(&rows)?;
        }
        DataCmd::Activity(a) => {
            let rows = data.activity(&a.address, a.limit, a.offset).await?;
            output::print_json(&rows)?;
        }
        DataCmd::Holders(a) => {
            let rows = data.holders(&a.market, a.limit).await?;
            output::print_json(&rows)?;
        }
        DataCmd::Traded(a) => {
            let r = data.traded(&a.address).await?;
            output::print_json(&r)?;
        }
        DataCmd::OpenInterest(a) => {
            let r = data.open_interest(&a.market).await?;
            output::print_json(&r)?;
        }
        DataCmd::LiveVolume(a) => {
            let r = data.live_volume(&a.id).await?;
            output::print_json(&r)?;
        }
        DataCmd::PricesHistory(a) => {
            let r = data
                .prices_history(&a.market, a.interval.as_deref(), a.fidelity)
                .await?;
            output::print_json(&r)?;
        }
        DataCmd::UserPnl(a) => {
            let r = data
                .user_pnl(&a.address, a.interval.as_deref(), a.fidelity.as_deref())
                .await?;
            output::print_json(&r)?;
        }
        DataCmd::Stats => {
            let r = data.stats().await?;
            output::print_json(&r)?;
        }
        DataCmd::Leaderboard(a) => {
            let r = data
                .leaderboard(
                    a.time_period.as_deref(),
                    a.order_by.as_deref(),
                    a.limit,
                    a.offset,
                )
                .await?;
            output::print_json(&r)?;
        }
        DataCmd::UnwrapRequests(a) => {
            let r = data.unwrap_requests(&a.safe, a.claimed).await?;
            output::print_json(&r)?;
        }
    }
    let _ = fmt; // table rendering deferred — JSON is always usable for now
    Ok(())
}
