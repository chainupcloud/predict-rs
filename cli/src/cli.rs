//! Clap definitions for the `pm` binary.

use clap::{Parser, Subcommand, ValueEnum};

use crate::output::Format;

#[derive(Debug, Parser)]
#[command(name = "pm", version, about = "ChainUp pm-cup2026 terminal client", long_about = None)]
pub struct Cli {
    /// REST endpoint for the chainup CLOB service.
    /// Defaults to the dev endpoint `https://clob-api.predict.prax1s.xyz`.
    #[arg(
        long,
        global = true,
        env = "PM_ENDPOINT",
        default_value = pm_rs_clob_client::DEFAULT_ENDPOINT
    )]
    pub endpoint: String,

    /// Multi-tenant `scopeId` (bytes32 hex). Empty = no scope.
    /// Only used by signing flows in Phase 2+, kept here so it can be threaded through.
    #[arg(long, global = true, env = "PM_SCOPE_ID", default_value = "")]
    pub scope_id: String,

    /// Chain ID for signing. Defaults to OP Sepolia (11155420).
    #[arg(long, global = true, env = "PM_CHAIN_ID", default_value_t = pm_rs_clob_client::OP_SEPOLIA)]
    pub chain_id: u64,

    /// Output format.
    #[arg(short = 'o', long, global = true, env = "PM_OUTPUT", default_value = "table")]
    pub output: Format,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// API health check (`GET /ok`).
    Ok,
    /// Server time (`GET /time`).
    Time,
    /// Mid-price for a token.
    Midpoint(TokenArgs),
    /// Best price for a token + side.
    Price(PriceArgs),
    /// Bid-ask spread for a token.
    Spread(TokenArgs),
    /// Order book snapshot.
    Book(TokenArgs),
    /// Tick size for a token.
    TickSize(TokenArgs),
    /// Fee rate (bps) for a token.
    FeeRate(TokenArgs),
    /// Last trade price for a token.
    LastTrade(TokenArgs),
}

#[derive(Debug, clap::Args)]
pub struct TokenArgs {
    pub token_id: String,
}

#[derive(Debug, clap::Args)]
pub struct PriceArgs {
    pub token_id: String,
    /// Side: `buy` or `sell`.
    #[arg(long, value_enum)]
    pub side: SideArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SideArg {
    Buy,
    Sell,
}

impl From<SideArg> for pm_rs_clob_client::types::Side {
    fn from(v: SideArg) -> Self {
        match v {
            SideArg::Buy => Self::Buy,
            SideArg::Sell => Self::Sell,
        }
    }
}
