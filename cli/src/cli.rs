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

    /// EOA private key (hex, with or without `0x` prefix) used to sign L1 EIP-712 challenges.
    /// Required by every Phase 2 subcommand. Prefer the env var over the flag — exposing a
    /// private key in shell history is unsafe. When absent, falls back to the value stored
    /// by `pm wallet create` / `pm wallet import` in `<config-dir>/config.toml`.
    #[arg(long, global = true, env = "PM_PRIVATE_KEY", hide_env_values = true)]
    pub private_key: Option<String>,

    /// Override the directory holding `config.toml` (default: `dirs::config_dir()/pm`,
    /// i.e. `~/.config/pm` on Linux). Used by `pm wallet …` for persistence and by every
    /// other command as the final fallback for `--private-key` / `--chain-id` / `--scope-id`.
    #[arg(long, global = true, env = "PM_CONFIG_DIR")]
    pub config_dir: Option<String>,

    /// CTFExchange contract address. Currently unused on Phase 2.1 paths (`order` flows land
    /// in Phase 2.2) but accepted up front so workflows that combine auth + order placement
    /// share the same env layout.
    #[arg(long, global = true, env = "PM_EXCHANGE_ADDRESS")]
    pub exchange_address: Option<String>,

    /// Pre-stored L2 credentials JSON file (matches the `/auth/api-key` response shape:
    /// `{"apiKey": "...", "secret": "...", "passphrase": "..."}`). When absent, L2 commands
    /// auto-derive via `GET /auth/derive-api-key` before issuing the real request.
    #[arg(long, global = true, env = "PM_CREDENTIALS_FILE")]
    pub credentials: Option<String>,

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
    /// Gamma metadata API (events / markets / tags / series / comments / profiles / search).
    Gamma(crate::gamma_commands::GammaArgs),
    /// L1 / L2 authentication: API-key management.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// `GET /balance-allowance` (or `/balance-allowance/update` with `--update`).
    Balance(BalanceArgs),
    /// WebSocket subscriptions (`/ws/market` public; `/ws/user` auth-required).
    Ws(WsArgs),
    /// Phase 2.2: order lifecycle (create / cancel / list / replace / scoring).
    #[command(subcommand)]
    Order(crate::order_commands::OrderCommand),
    /// `GET /trades` — paginated trade history (L2-auth).
    Trade(crate::order_commands::TradeArgs),
    /// `POST /heartbeats` — maker-program heartbeat ping.
    Heartbeat,
    /// Local wallet / config-file management (create / import / address / show / reset).
    #[command(subcommand)]
    Wallet(crate::wallet_commands::WalletCommand),
}

#[derive(Debug, clap::Args)]
pub struct WsArgs {
    #[command(subcommand)]
    pub command: WsCmd,
}

#[derive(Debug, Subcommand)]
pub enum WsCmd {
    /// Connect, send one PING, verify the server accepts the upgrade, disconnect.
    Ping,
    /// Subscribe to `/ws/market` and print N book frames (then exit).
    Book(WsBookArgs),
    /// Subscribe to `/ws/market` and stream updates until Ctrl-C.
    BookWatch(WsBookWatchArgs),
    /// Subscribe to `/ws/user` (auth-required) and stream order / trade updates.
    User(WsUserArgs),
}

#[derive(Debug, clap::Args)]
pub struct WsBookArgs {
    /// Asset (token) IDs to subscribe to. One or more.
    #[arg(required = true, num_args = 1..)]
    pub asset_ids: Vec<String>,
    /// Disable the initial book snapshot dump.
    #[arg(long)]
    pub no_initial_dump: bool,
    /// Order-book depth (1 / 2 / 3). Default = server default (2).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
    pub level: Option<u8>,
    /// Enable the optional `best_bid_ask` / `new_market` / `market_resolved` events.
    #[arg(long)]
    pub custom_features: bool,
    /// Stop after this many events arrive.
    #[arg(long, default_value_t = 1)]
    pub count: u32,
}

#[derive(Debug, clap::Args)]
pub struct WsBookWatchArgs {
    /// Asset (token) ID to watch.
    pub asset_id: String,
    /// `--print-as-json` prints raw event JSON per line.
    #[arg(long, group = "watch_fmt")]
    pub print_as_json: bool,
    /// `--print-as-table` (default) prints a compact `BID / ASK` ticker.
    #[arg(long, group = "watch_fmt")]
    pub print_as_table: bool,
}

#[derive(Debug, clap::Args)]
pub struct WsUserArgs {
    /// Optional condition IDs (markets) to filter by. Empty = all owned markets.
    #[arg(long = "market")]
    pub markets: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// `POST /auth/api-key` — create a new L2 API key for the signer.
    CreateKey(CreateKeyArgs),
    /// `GET /auth/derive-api-key` — recover the credentials for an existing key.
    DeriveKey(DeriveKeyArgs),
    /// `DELETE /auth/api-key` — revoke the L2 key for `(signer, scope, nonce)`.
    DeleteKey(DeleteKeyArgs),
    /// `GET /auth/api-keys` — list active API keys + chainup `proxy_wallet` (L2-auth).
    ListKeys,
}

#[derive(Debug, clap::Args)]
pub struct CreateKeyArgs {
    /// Nonce embedded in the `ClobAuth` EIP-712 message (default 0).
    #[arg(long, default_value_t = 0)]
    pub nonce: u32,
    /// Signature type — accepted up-front so the same env layout can be reused by the
    /// order-placement subcommands that land in Phase 2.2. The L1 server does not read this.
    #[arg(long, value_enum, default_value = "gnosis-safe")]
    pub signature_type: SignatureTypeArg,
    /// Optional funder address for proxy / Safe flows — see `--signature-type`. Not consumed
    /// by Phase 2.1 paths.
    #[arg(long)]
    pub funder: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct DeriveKeyArgs {
    #[arg(long, default_value_t = 0)]
    pub nonce: u32,
}

#[derive(Debug, clap::Args)]
pub struct DeleteKeyArgs {
    /// API-key UUID (accepted for symmetry with rs-clob-client; the server identifies the
    /// row by `(address, scope, nonce)`).
    pub key: String,
    #[arg(long, default_value_t = 0)]
    pub nonce: u32,
}

#[derive(Debug, clap::Args)]
pub struct BalanceArgs {
    /// Asset class: `collateral` (USDC) or `conditional` (outcome token).
    #[arg(long, value_enum)]
    pub asset_type: AssetTypeArg,
    /// Token ID — required iff `--asset-type conditional`.
    #[arg(long)]
    pub token: Option<String>,
    /// Use `GET /balance-allowance/update` (force subgraph refresh) instead of the cached
    /// `/balance-allowance`.
    #[arg(long)]
    pub update: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SignatureTypeArg {
    /// signatureType=0 — direct EOA signing.
    Eoa,
    /// signatureType=1 — Polymarket proxy wallet.
    Proxy,
    /// signatureType=2 — Gnosis Safe (1-of-1) — chainup default.
    GnosisSafe,
}

impl From<SignatureTypeArg> for pm_rs_clob_client::types::SignatureType {
    fn from(v: SignatureTypeArg) -> Self {
        match v {
            SignatureTypeArg::Eoa => Self::Eoa,
            SignatureTypeArg::Proxy => Self::PolyProxy,
            SignatureTypeArg::GnosisSafe => Self::PolyGnosisSafe,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AssetTypeArg {
    Collateral,
    Conditional,
}

impl From<AssetTypeArg> for pm_rs_clob_client::AssetType {
    fn from(v: AssetTypeArg) -> Self {
        match v {
            AssetTypeArg::Collateral => Self::Collateral,
            AssetTypeArg::Conditional => Self::Conditional,
        }
    }
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
