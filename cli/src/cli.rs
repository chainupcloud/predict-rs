//! Clap definitions for the `pm` binary.

use clap::{Parser, Subcommand, ValueEnum};

use crate::output::Format;

#[derive(Debug, Parser)]
#[command(name = "pm", version, about = "ChainUp pm-cup2026 terminal client", long_about = None)]
pub struct Cli {
    /// Tenant root host. The CLOB / Gamma / WebSocket endpoints are derived using the canonical
    /// chainup subdomain pattern (`clob-api.<host>` / `gamma-api.<host>` / `clob-ws.<host>`).
    /// Either this OR `--clob-endpoint` must be supplied — they are mutually exclusive.
    #[arg(long, global = true, env = "PM_TENANT", conflicts_with = "clob_endpoint")]
    pub tenant: Option<String>,

    /// CLOB REST endpoint. Overrides the derived URL when `--tenant` is also set with `--gamma-endpoint`
    /// / `--ws-endpoint`; if `--tenant` is absent, this is the only required endpoint flag.
    #[arg(long, global = true, env = "PM_CLOB_ENDPOINT")]
    pub clob_endpoint: Option<String>,

    /// Gamma REST endpoint. Defaults to `gamma-api.<tenant>` when `--tenant` is provided.
    #[arg(long, global = true, env = "PM_GAMMA_ENDPOINT")]
    pub gamma_endpoint: Option<String>,

    /// CLOB WebSocket endpoint. Defaults to `wss://clob-ws.<tenant>` when `--tenant` is provided.
    #[arg(long, global = true, env = "PM_WS_ENDPOINT")]
    pub ws_endpoint: Option<String>,

    /// Multi-tenant `scopeId` (`bytes32` hex). Empty = no scope. Threaded through signing flows in Phase 2+.
    #[arg(long, global = true, env = "PM_SCOPE_ID", default_value = "")]
    pub scope_id: String,

    /// Chain id for signing. Required by Phase 2+ flows; Phase 1 read-only commands ignore it.
    #[arg(long, global = true, env = "PM_CHAIN_ID")]
    pub chain_id: Option<u64>,

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
    /// Print the resolved endpoint configuration (debugging).
    Endpoints,
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
