//! Clap definitions for the `predict-cli` binary.

use clap::{Parser, Subcommand, ValueEnum};

use crate::output::Format;

#[derive(Debug, Parser)]
#[command(name = "predict-cli", version, about = "Prediction market terminal client", long_about = None)]
pub struct Cli {
    /// Built-in network to use (selects chain id, endpoints, and all contract addresses).
    /// Defaults to `monad` when neither this flag nor `config.toml` sets one. Run
    /// `predict-cli endpoints` to see what a selection resolves to.
    #[arg(long, global = true, env = "PM_NETWORK")]
    pub network: Option<String>,

    /// Tenant root host override. The CLOB / Gamma / WebSocket endpoints are derived from this
    /// using the canonical subdomain pattern (`clob-api.<host>` / `gamma-api.<host>` /
    /// `clob-ws.<host>`). Resolution order: this flag > `config.toml` `tenant` > the selected
    /// network's domain (e.g. `hermestrade.xyz` for `monad`). Set it for the
    /// same-network / different-tenant case. Mutually exclusive with `--clob-endpoint`.
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

    /// Multi-tenant `scopeId` (`bytes32` hex). Empty = no scope. Threaded through signing flows.
    #[arg(long, global = true, env = "PM_SCOPE_ID", default_value = "")]
    pub scope_id: String,

    /// Chain id for signing. Required by signing flows; read-only commands ignore it.
    #[arg(long, global = true, env = "PM_CHAIN_ID")]
    pub chain_id: Option<u64>,

    /// EOA private key (hex, with or without `0x` prefix) used to sign EIP-712 challenges and
    /// orders. When absent, falls back to the key stored in `<config-dir>/config.toml` by
    /// `predict-cli wallet create` / `wallet import` / `setup`. The `PM_PRIVATE_KEY` env var is
    /// intentionally NOT supported — a private key in the environment leaks via
    /// `/proc/<pid>/environ` and to child processes. Pass the flag or use the config file.
    #[arg(long, global = true)]
    pub private_key: Option<String>,

    /// Override the directory holding `config.toml` (default: `dirs::config_dir()/predict`,
    /// i.e. `~/.config/predict` on Linux). Holds the private key (mode 0600) and is the fallback
    /// source for `--private-key` / `--chain-id` / `--scope-id` / `--network`.
    #[arg(long, global = true, env = "PM_CONFIG_DIR")]
    pub config_dir: Option<String>,

    /// Global EIP-712 signature type (`eoa` / `proxy` / `gnosis-safe`). Defaults to
    /// `gnosis-safe` — Safe-wallet flow where the EOA signs but the Safe holds
    /// funds and is the `maker`. Persisted by `predict-cli wallet create / import / set-safe`;
    /// the flag overrides the stored value for the current invocation only.
    #[arg(long, global = true, env = "PM_SIGNATURE_TYPE")]
    pub signature_type: Option<SignatureTypeArg>,

    /// CTFExchange contract address. Accepted up front so workflows that combine auth + order placement
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
    /// Batch midpoints — `POST /midpoints`.
    Midpoints(TokensArgs),
    /// Batch prices — `POST /prices`. Tokens take the form `<id>:<side>`.
    Prices(PricesBatchArgs),
    /// Batch spreads — `POST /spreads`.
    Spreads(TokensArgs),
    /// Batch order books — `POST /books`. Tokens take the form `<id>:<side>`.
    Books(PricesBatchArgs),
    /// Batch last trade prices — `POST /last-trades-prices` (server cap 500).
    LastTrades(TokensArgs),
    /// `GET /price-history` — OHLCV-style price series for one token.
    PriceHistory(PriceHistoryArgs),
    /// Print the resolved endpoint configuration (debugging).
    Endpoints,
    /// Gamma metadata API (events / markets / tags / series / comments / profiles / search).
    Gamma(crate::gamma_commands::GammaArgs),
    /// Data API (portfolio / trades / activity / leaderboards) — wraps `data-service`.
    Data(crate::data_commands::DataArgs),
    /// L1 / L2 authentication: API-key management.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// `GET /balance-allowance` (or `/balance-allowance/update` with `--update`).
    Balance(BalanceArgs),
    /// WebSocket subscriptions (`/ws/market` public; `/ws/user` auth-required).
    Ws(WsArgs),
    /// Order lifecycle (create / cancel / list / replace / scoring).
    #[command(subcommand)]
    Order(crate::order_commands::OrderCommand),
    /// `GET /trades` — paginated trade history (L2-auth).
    Trade(crate::order_commands::TradeArgs),
    /// `POST /heartbeats` — maker-program heartbeat ping.
    Heartbeat,
    /// Local wallet / config-file management (create / import / address / show / reset).
    #[command(subcommand)]
    Wallet(crate::wallet_commands::WalletCommand),
    /// On-chain approval lookup (read-only). Mirrors the upstream CLI's `approve check`.
    #[command(subcommand)]
    Approve(crate::approve_commands::ApproveCommand),
    /// Interactive REPL. Mirrors the upstream CLI's `shell` — each line parses as a fresh
    /// `predict-cli <args>` invocation; env vars / config-file state apply per line.
    Shell,
    /// Guided first-time setup wizard (wallet + tenant + Safe + L2 API key).
    /// Mirrors the upstream CLI's `setup`, adapted for multi-tenant topology.
    Setup,
    /// Conditional Token Framework helpers — pure off-chain ID calculations
    /// (`condition-id` / `position-id`), RPC-backed `collection-id`, and Safe-mode
    /// on-chain writes (`redeem` / `split` / `merge`) via the relayer.
    Ctf(crate::ctf_commands::CtfArgs),
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
    /// `GET /auth/api-keys` — list active API keys + `proxy_wallet` (L2-auth).
    ListKeys,
}

#[derive(Debug, clap::Args)]
pub struct CreateKeyArgs {
    /// Nonce embedded in the `ClobAuth` EIP-712 message (default 0).
    #[arg(long, default_value_t = 0)]
    pub nonce: u32,
    /// Optional funder address for proxy / Safe flows.
    /// See the global `--signature-type` for the EIP-712 signing mode.
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
    /// signatureType=1 — upstream V1 proxy wallet.
    Proxy,
    /// signatureType=2 — Gnosis Safe (1-of-1) — default.
    GnosisSafe,
}

impl From<SignatureTypeArg> for predict_rs_clob_client::types::SignatureType {
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

impl From<AssetTypeArg> for predict_rs_clob_client::AssetType {
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

/// Batch read endpoints (`predict-cli midpoints / spreads / last-trades`) — accept N positional token ids.
#[derive(Debug, clap::Args)]
pub struct TokensArgs {
    /// One or more token ids. Pass as separate args (`predict-cli midpoints t1 t2 t3`) or as a single
    /// comma-separated string (`predict-cli midpoints t1,t2,t3`).
    #[arg(required = true, num_args = 1..)]
    pub token_ids: Vec<String>,
}

/// `predict-cli prices` / `predict-cli books` — each token must specify a side: `<id>:buy` or `<id>:sell`.
#[derive(Debug, clap::Args)]
pub struct PricesBatchArgs {
    /// Tokens in `<id>:<buy|sell>` form. Repeat the flag-free positional arg for each entry.
    #[arg(required = true, num_args = 1..)]
    pub entries: Vec<String>,
}

#[derive(Debug, clap::Args)]
pub struct PriceHistoryArgs {
    pub token_id: String,
    /// Interval bucket size. No `1m` minute granularity.
    #[arg(long, value_enum, default_value = "1d")]
    pub interval: PriceHistoryIntervalArg,
    /// Optional override on the server's default samples-per-bucket (minutes per point).
    #[arg(long)]
    pub fidelity: Option<u32>,
    /// Optional limit on the number of points returned (server cap 10000, default 2000).
    #[arg(long)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PriceHistoryIntervalArg {
    #[value(name = "1h")]
    H1,
    #[value(name = "6h")]
    H6,
    #[value(name = "1d")]
    D1,
    #[value(name = "1w")]
    W1,
    #[value(name = "1m")]
    M1,
    All,
}

impl From<PriceHistoryIntervalArg> for predict_rs_clob_client::clob::types::PriceHistoryInterval {
    fn from(v: PriceHistoryIntervalArg) -> Self {
        use predict_rs_clob_client::clob::types::PriceHistoryInterval as I;
        match v {
            PriceHistoryIntervalArg::H1 => I::H1,
            PriceHistoryIntervalArg::H6 => I::H6,
            PriceHistoryIntervalArg::D1 => I::D1,
            PriceHistoryIntervalArg::W1 => I::W1,
            PriceHistoryIntervalArg::M1 => I::M1,
            PriceHistoryIntervalArg::All => I::All,
        }
    }
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

impl From<SideArg> for predict_rs_clob_client::types::Side {
    fn from(v: SideArg) -> Self {
        match v {
            SideArg::Buy => Self::Buy,
            SideArg::Sell => Self::Sell,
        }
    }
}
