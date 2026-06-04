//! Command dispatch.

use anyhow::{Context, anyhow};
use chrono::DateTime;
use predict_rs_clob_client::clob::types::OrderBookSummary;
use predict_rs_clob_client::types::ScopeId;
use predict_rs_clob_client::{
    ApiKeyInfo, AssetType, BalanceAllowanceResponse, Client, ClientBuilder, Credentials, Endpoints,
    PMCup26Signer,
};
use secrecy::ExposeSecret;
use tabled::Tabled;
use url::Url;

use crate::cli::{AuthCommand, BalanceArgs, Cli, Command, CreateKeyArgs, DeleteKeyArgs};
use crate::output::{self, Format};

pub async fn run(args: Cli) -> anyhow::Result<()> {
    // `predict-cli wallet …` is local-only — no endpoint required. Dispatch before endpoint
    // resolution so the wallet UX works on a fresh checkout with no flags set.
    if matches!(args.command, Command::Wallet(_)) {
        let mut owned = args;
        let fmt = owned.output;
        let sub = match std::mem::replace(&mut owned.command, Command::Ok) {
            Command::Wallet(w) => w,
            _ => unreachable!(),
        };
        return crate::wallet_commands::run(&owned, &sub, fmt).await;
    }

    // `predict-cli shell` is purely local — no endpoint required. Dispatch before endpoint
    // resolution so users can launch the REPL without any `--tenant` flag.
    if matches!(args.command, Command::Shell) {
        return crate::shell_commands::run().await;
    }

    // `predict-cli setup` runs its own interactive flow; some sub-steps build a Client of their
    // own. Dispatch before endpoint resolution so a fresh install can run `predict-cli setup`.
    if matches!(args.command, Command::Setup) {
        return crate::setup_commands::run(&args).await;
    }

    // `predict-cli ctf` — mix of off-chain helpers (`condition-id`, `position-id`) and on-chain
    // Safe-mode writes (`redeem` / `split` / `merge`). The on-chain variants resolve the
    // selected `--network` (default monad) for contracts + relayer, so no CLOB endpoint is
    // needed at the top level.
    if matches!(args.command, Command::Ctf(_)) {
        let mut owned = args;
        let fmt = owned.output;
        let cargs = match std::mem::replace(&mut owned.command, Command::Ok) {
            Command::Ctf(c) => c,
            _ => unreachable!(),
        };
        return crate::ctf_commands::run(&owned, cargs, fmt).await;
    }

    // `predict-cli approve …` only touches the on-chain RPC — no CLOB endpoint required.
    if matches!(args.command, Command::Approve(_)) {
        let mut owned = args;
        let fmt = owned.output;
        let sub = match std::mem::replace(&mut owned.command, Command::Ok) {
            Command::Approve(a) => a,
            _ => unreachable!(),
        };
        return crate::approve_commands::run(&owned, &sub, fmt).await;
    }

    // `predict-cli deposit` broadcasts a direct EOA tx (wrap) against the network RPC — no CLOB
    // endpoint required, so dispatch before endpoint resolution.
    if matches!(args.command, Command::Deposit(_)) {
        let mut owned = args;
        let fmt = owned.output;
        let dargs = match std::mem::replace(&mut owned.command, Command::Ok) {
            Command::Deposit(d) => d,
            _ => unreachable!(),
        };
        return crate::wusd_commands::run_deposit(&owned, &dargs, fmt).await;
    }

    // `predict-cli withdraw …` — initiate (Safe meta-tx via relayer) + claim (direct EOA tx);
    // no CLOB endpoint required, so dispatch before endpoint resolution.
    if matches!(args.command, Command::Withdraw(_)) {
        let mut owned = args;
        let fmt = owned.output;
        let sub = match std::mem::replace(&mut owned.command, Command::Ok) {
            Command::Withdraw(w) => w,
            _ => unreachable!(),
        };
        return crate::wusd_commands::run_withdraw(&owned, &sub, fmt).await;
    }

    let endpoints = resolve_endpoints(&args)?;
    let mut builder = Client::builder().endpoints(endpoints);
    if let Some(cid) = effective_chain_id(&args)? {
        builder = builder.chain_id(cid);
    }

    // The unauthenticated client is used for every read path and for L1 auth
    // (POST/GET /auth/api-key); L2 paths re-build a client with credentials attached.
    let client = builder.build().context("build client")?;
    let fmt = args.output;

    // `gamma_commands::run` consumes its `GammaArgs` by value. Handle it before the borrowing
    // match below so we don't fight the borrow checker when other arms need `&args`.
    if matches!(args.command, Command::Gamma(_)) {
        let Cli {
            command: Command::Gamma(gargs),
            ..
        } = args
        else {
            unreachable!("matches! guarded above")
        };
        return crate::gamma_commands::run(client, fmt, gargs).await;
    }
    if matches!(args.command, Command::Data(_)) {
        let Cli {
            command: Command::Data(dargs),
            ..
        } = args
        else {
            unreachable!("matches! guarded above")
        };
        return crate::data_commands::run(client, fmt, dargs).await;
    }
    if matches!(args.command, Command::Ws(_)) {
        // `ws_commands::run` consumes `WsArgs`; reconstruct an args struct that holds the
        // rest for the helpers in `ws_commands` that re-resolve endpoints.
        let mut owned = args;
        let wargs = match std::mem::replace(&mut owned.command, Command::Ok) {
            Command::Ws(w) => w,
            _ => unreachable!(),
        };
        return crate::ws_commands::run(owned, fmt, wargs).await;
    }

    match &args.command {
        Command::Endpoints => {
            print_endpoints(&args, &client, fmt)?;
        }
        Command::Ok => {
            let body = client.ok().await?;
            output::print_scalar("status", body.trim(), fmt)?;
        }
        Command::Time => {
            let ts = client.time().await?;
            let human = DateTime::from_timestamp(ts, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| String::from("(invalid)"));
            match fmt {
                Format::Json => output::print_json(
                    &serde_json::json!({ "unix": ts, "iso": human }),
                )?,
                Format::Table => println!("unix: {ts}\niso : {human}"),
            }
        }
        Command::Midpoint(a) => {
            let resp = client.midpoint(&a.token_id).await?;
            output::print_scalar("midpoint", resp.price, fmt)?;
        }
        Command::Price(a) => {
            let resp = client.price(&a.token_id, a.side.into()).await?;
            output::print_scalar("price", resp.price, fmt)?;
        }
        Command::Spread(a) => {
            let resp = client.spread(&a.token_id).await?;
            output::print_scalar("spread", resp.spread, fmt)?;
        }
        Command::Book(a) => {
            let resp = client.book(&a.token_id).await?;
            print_book(&resp, fmt)?;
        }
        Command::TickSize(a) => {
            let resp = client.tick_size(&a.token_id).await?;
            output::print_scalar("minimum_tick_size", resp.minimum_tick_size, fmt)?;
        }
        Command::FeeRate(a) => {
            let resp = client.fee_rate(&a.token_id).await?;
            output::print_scalar("fee_rate_bps", resp.fee_rate_bps, fmt)?;
        }
        Command::LastTrade(a) => {
            let resp = client.last_trade_price(&a.token_id).await?;
            output::print_scalar("last_trade_price", resp.price, fmt)?;
        }
        Command::Midpoints(a) => {
            let ids = expand_csv(&a.token_ids);
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            let resp = client.midpoints(&refs).await?;
            output::print_json(&serde_json::to_value(&resp)?)?;
        }
        Command::Spreads(a) => {
            let ids = expand_csv(&a.token_ids);
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            let resp = client.spreads(&refs).await?;
            output::print_json(&serde_json::to_value(&resp)?)?;
        }
        Command::Prices(a) => {
            let requests = parse_token_side_entries(&a.entries)?;
            let resp = client.prices(&requests).await?;
            output::print_json(&serde_json::to_value(&resp)?)?;
        }
        Command::Books(a) => {
            let requests = parse_token_side_entries(&a.entries)?;
            let resp = client.books(&requests).await?;
            output::print_json(&serde_json::to_value(
                resp.iter()
                    .map(|maybe| match maybe {
                        Some(b) => serde_json::json!({
                            "asset_id": b.asset_id,
                            "timestamp": b.timestamp,
                            "hash": b.hash,
                            "bids": b.bids.iter().map(|l| serde_json::json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
                            "asks": b.asks.iter().map(|l| serde_json::json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
                        }),
                        None => serde_json::Value::Null,
                    })
                    .collect::<Vec<_>>(),
            )?)?;
        }
        Command::LastTrades(a) => {
            let ids = expand_csv(&a.token_ids);
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            let resp = client.last_trades_prices(&refs).await?;
            output::print_json(&serde_json::to_value(
                resp.iter()
                    .map(|e| serde_json::json!({
                        "token_id": e.token_id,
                        "price": e.price,
                        "side": e.side,
                    }))
                    .collect::<Vec<_>>(),
            )?)?;
        }
        Command::PriceHistory(a) => {
            let resp = client
                .price_history(&a.token_id, a.interval.into(), a.fidelity, a.limit)
                .await?;
            output::print_json(&serde_json::to_value(
                resp.history.iter().map(|p| serde_json::json!({"t": p.t, "p": p.p})).collect::<Vec<_>>(),
            )?)?;
        }
        Command::Gamma(_) => unreachable!("handled by early-return above"),
        Command::Data(_) => unreachable!("handled by early-return above"),
        Command::Ws(_) => unreachable!("handled by early-return above"),
        Command::Auth(sub) => run_auth(&args, sub, fmt).await?,
        Command::Balance(a) => run_balance(&args, a, fmt).await?,
        Command::Order(sub) => crate::order_commands::run(&args, sub, fmt).await?,
        Command::Trade(a) => crate::order_commands::run_trade(&args, a, fmt).await?,
        Command::Heartbeat => crate::order_commands::run_heartbeat(&args, fmt).await?,
        Command::Wallet(_) => unreachable!("handled by early-return above"),
        Command::Approve(_) => unreachable!("handled by early-return above"),
        Command::Shell => unreachable!("handled by early-return above"),
        Command::Setup => unreachable!("handled by early-return above"),
        Command::Ctf(_) => unreachable!("handled by early-return above"),
        Command::Deposit(_) => unreachable!("handled by early-return above"),
        Command::Withdraw(_) => unreachable!("handled by early-return above"),
    }
    Ok(())
}

// ─── Auth / balance dispatch ─────────────────────────────────

async fn run_auth(args: &Cli, sub: &AuthCommand, fmt: Format) -> anyhow::Result<()> {
    match sub {
        AuthCommand::CreateKey(a) => {
            let signer = signer_from_args(args)?;
            let client = build_l1_client(args)?;
            let CreateKeyArgs { nonce, funder } = a;
            let _ = funder; // accepted for forward compatibility
            let creds = client.create_api_key(&signer, Some(*nonce)).await?;
            print_credentials(&creds, fmt)?;
        }
        AuthCommand::DeriveKey(a) => {
            let signer = signer_from_args(args)?;
            let client = build_l1_client(args)?;
            let creds = client.derive_api_key(&signer, Some(a.nonce)).await?;
            print_credentials(&creds, fmt)?;
        }
        AuthCommand::DeleteKey(a) => {
            let DeleteKeyArgs { key, nonce } = a;
            let signer = signer_from_args(args)?;
            let client = build_l1_client(args)?;
            let uuid = key
                .parse::<uuid::Uuid>()
                .with_context(|| format!("invalid API-key UUID '{key}'"))?;
            delete_with_nonce(&client, &signer, uuid, *nonce).await?;
        }
        AuthCommand::ListKeys => {
            let info = with_l2_credentials(args, |c| async move { c.api_keys().await }).await?;
            print_api_keys(&info, fmt)?;
        }
    }
    Ok(())
}

async fn run_balance(args: &Cli, a: &BalanceArgs, fmt: Format) -> anyhow::Result<()> {
    let asset: AssetType = a.asset_type.into();
    let token: Option<String> = a.token.clone();
    let update = a.update;
    let resp = with_l2_credentials(args, move |client| async move {
        let token_ref = token.as_deref();
        if update {
            client.update_balance_allowance(asset, token_ref).await
        } else {
            client.balance_allowance(asset, token_ref).await
        }
    })
    .await?;
    print_balance(&resp, fmt)?;
    Ok(())
}

// ─── helpers: signer / credentials / client builders ────────────────────

pub(crate) fn signer_from_args(args: &Cli) -> anyhow::Result<PMCup26Signer> {
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let pk_owned: String;
    let pk: &str = if let Some(p) = args.private_key.as_deref() {
        p
    } else if let Some(p) = stored.as_ref().and_then(|c| c.private_key.as_deref()) {
        pk_owned = p.to_owned();
        &pk_owned
    } else {
        return Err(anyhow!(
            "private key required: pass --private-key or store one with `predict-cli wallet create` / `wallet import` / `setup` (the PM_PRIVATE_KEY env var is intentionally not supported)"
        ));
    };

    let chain_id = effective_chain_id_with(args, stored.as_ref())?
        .ok_or_else(|| anyhow!("chain id required for L1 auth: pass --chain-id or set PM_CHAIN_ID"))?;

    let mut signer = PMCup26Signer::from_hex(pk, chain_id)?;
    let scope_hex = effective_scope_id(args, stored.as_ref());
    if !scope_hex.is_empty() {
        let scope = ScopeId::from_hex(&scope_hex)
            .with_context(|| format!("invalid scope id '{scope_hex}'"))?;
        signer = signer.with_scope_id(scope);
    }
    // Exchange (EIP-712 Order `verifyingContract`): explicit flag wins, else the selected
    // network's `ctf_exchange`. Always set it so order signing works without a per-command flag.
    let exchange_hex = match &args.exchange_address {
        Some(h) => h.clone(),
        None => {
            crate::networks::get(&crate::networks::effective_network_name(args, stored.as_ref()))?
                .contracts
                .ctf_exchange
        }
    };
    let exchange = parse_address(&exchange_hex)
        .with_context(|| format!("invalid exchange address '{exchange_hex}'"))?;
    signer = signer.with_exchange(exchange);
    Ok(signer)
}

pub(crate) fn effective_chain_id(args: &Cli) -> anyhow::Result<Option<u64>> {
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    effective_chain_id_with(args, stored.as_ref())
}

/// Resolve the EIP-712 signature type for the current invocation. Order: global flag /
/// env > stored `config.toml` > default `gnosis-safe` (Safe-wallet flow).
pub(crate) fn effective_signature_type(
    args: &Cli,
) -> anyhow::Result<predict_rs_clob_client::types::SignatureType> {
    use crate::cli::SignatureTypeArg;
    if let Some(s) = args.signature_type {
        return Ok(s.into());
    }
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let parsed = match stored.as_ref().and_then(|c| c.signature_type.as_deref()) {
        Some("eoa") => Some(SignatureTypeArg::Eoa),
        Some("proxy") => Some(SignatureTypeArg::Proxy),
        Some("gnosis-safe") => Some(SignatureTypeArg::GnosisSafe),
        Some(other) => {
            return Err(anyhow!(
                "config.toml: unrecognised signature_type '{other}' (expected eoa|proxy|gnosis-safe)"
            ));
        }
        None => None,
    };
    Ok(parsed.unwrap_or(SignatureTypeArg::GnosisSafe).into())
}

fn effective_chain_id_with(
    args: &Cli,
    stored: Option<&crate::config_store::StoredConfig>,
) -> anyhow::Result<Option<u64>> {
    if let Some(c) = args.chain_id.or_else(|| stored.and_then(|c| c.chain_id)) {
        return Ok(Some(c));
    }
    // Fall back to the selected network's chain id so signing works without `--chain-id`.
    let net = crate::networks::get(&crate::networks::effective_network_name(args, stored))?;
    Ok(Some(net.network.chain_id))
}

fn effective_scope_id(args: &Cli, stored: Option<&crate::config_store::StoredConfig>) -> String {
    if !args.scope_id.is_empty() {
        return args.scope_id.clone();
    }
    stored
        .and_then(|c| c.scope_id.clone())
        .unwrap_or_default()
}

fn build_l1_client(args: &Cli) -> anyhow::Result<Client> {
    let endpoints = resolve_endpoints(args)?;
    let mut b = Client::builder().endpoints(endpoints);
    if let Some(cid) = effective_chain_id(args)? {
        b = b.chain_id(cid);
    }
    b.build().context("build client")
}

fn build_l2_client(args: &Cli, creds: Credentials, signer: &PMCup26Signer) -> anyhow::Result<Client> {
    let endpoints = resolve_endpoints(args)?;
    let mut b: ClientBuilder = Client::builder().endpoints(endpoints);
    if let Some(cid) = effective_chain_id(args)? {
        b = b.chain_id(cid);
    }
    Ok(b.credentials(creds).signer_address(signer.address()).build()?)
}

pub(crate) async fn with_l2_credentials<F, Fut, T>(args: &Cli, op: F) -> anyhow::Result<T>
where
    F: FnOnce(Client) -> Fut,
    Fut: std::future::Future<Output = predict_rs_clob_client::Result<T>>,
{
    let signer = signer_from_args(args)?;
    // Try the credentials file first, else auto-derive.
    let creds = match args.credentials.as_ref() {
        Some(p) => load_credentials_file(p)?,
        None => {
            // Idempotent: re-derive (or create) using the signer + scope.
            let bootstrap = build_l1_client(args)?;
            bootstrap.create_or_derive_api_key(&signer, None).await?
        }
    };
    let client = build_l2_client(args, creds, &signer)?;
    Ok(op(client).await?)
}

fn load_credentials_file(path: &str) -> anyhow::Result<Credentials> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read --credentials file {path}"))?;
    serde_json::from_str::<Credentials>(&raw)
        .with_context(|| format!("decode credentials file {path}"))
}

pub(crate) fn parse_address(s: &str) -> anyhow::Result<predict_rs_clob_client::types::Address> {
    use std::str::FromStr;
    predict_rs_clob_client::types::Address::from_str(s)
        .map_err(|e| anyhow!("parse address: {e}"))
}

async fn delete_with_nonce(
    client: &Client,
    signer: &PMCup26Signer,
    _key: uuid::Uuid,
    nonce: u32,
) -> anyhow::Result<()> {
    client
        .delete_api_key_with_nonce(signer, uuid::Uuid::nil(), nonce)
        .await?;
    Ok(())
}

// ─── printing ───────────────────────────────────────────────────────────

fn print_credentials(creds: &Credentials, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "apiKey": creds.key.to_string(),
            "secret": creds.secret().expose_secret(),
            "passphrase": creds.passphrase().expose_secret(),
        }))?,
        Format::Table => {
            println!("apiKey    : {}", creds.key);
            println!("secret    : {}", creds.secret().expose_secret());
            println!("passphrase: {}", creds.passphrase().expose_secret());
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct ApiKeyRow {
    field: &'static str,
    value: String,
}

fn print_api_keys(info: &ApiKeyInfo, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "apiKeys": info.api_keys,
            "address": info.address,
            "proxy_wallet": info.proxy_wallet,
        }))?,
        Format::Table => {
            let mut rows: Vec<ApiKeyRow> = vec![
                ApiKeyRow {
                    field: "address",
                    value: info.address.clone().unwrap_or_else(|| "(none)".into()),
                },
                ApiKeyRow {
                    field: "proxy_wallet",
                    value: info.proxy_wallet.clone().unwrap_or_else(|| "(none)".into()),
                },
            ];
            for (i, k) in info.api_keys.iter().enumerate() {
                rows.push(ApiKeyRow {
                    field: if i == 0 { "apiKeys[0]" } else { "apiKeys[n]" },
                    value: k.clone(),
                });
            }
            output::print_table(rows);
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct BalanceRow {
    field: &'static str,
    value: String,
}

fn print_balance(resp: &BalanceAllowanceResponse, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "balance": resp.balance,
            "allowances": resp.allowances,
            "virtual_available": resp.virtual_available,
            "locked": resp.locked,
        }))?,
        Format::Table => {
            let mut rows = vec![
                BalanceRow { field: "balance", value: resp.balance.clone() },
            ];
            if let Some(va) = &resp.virtual_available {
                rows.push(BalanceRow { field: "virtual_available", value: va.clone() });
            }
            if let Some(lk) = &resp.locked {
                rows.push(BalanceRow { field: "locked", value: lk.clone() });
            }
            for (spender, amount) in &resp.allowances {
                rows.push(BalanceRow {
                    field: "allowance",
                    value: format!("{spender} -> {amount}"),
                });
            }
            output::print_table(rows);
        }
    }
    Ok(())
}

/// Public alias used by sibling modules (e.g. `ws_commands`) so endpoint
/// resolution stays single-sourced here.
pub fn resolve_endpoints_pub(args: &Cli) -> anyhow::Result<Endpoints> {
    resolve_endpoints(args)
}

fn resolve_endpoints(args: &Cli) -> anyhow::Result<Endpoints> {
    // An explicit `--clob-endpoint` always wins (advanced override against a single host).
    let mut ep = if let Some(clob) = &args.clob_endpoint {
        Endpoints::clob_only(clob).with_context(|| format!("parse clob endpoint {clob}"))?
    } else {
        // Otherwise derive every endpoint from the effective tenant host. For the selected
        // network's own domain this yields the same clob / gamma / ws / data hosts the network
        // declares (canonical `clob-api.<host>` etc.).
        let host = effective_tenant(args)?;
        Endpoints::from_tenant(&host)
            .with_context(|| format!("derive endpoints from tenant {host}"))?
    };
    if let Some(g) = &args.gamma_endpoint {
        ep = ep.with_gamma(parse_endpoint(g)?);
    }
    if let Some(w) = &args.ws_endpoint {
        ep = ep.with_ws(parse_endpoint(w)?);
    }
    Ok(ep)
}

/// Resolve the tenant host for endpoint derivation. Order: `--tenant` flag / env >
/// `config.toml` `tenant` > the selected network's domain (e.g. `hermestrade.xyz` for `monad`).
fn effective_tenant(args: &Cli) -> anyhow::Result<String> {
    if let Some(t) = args.tenant.clone().filter(|s| !s.is_empty()) {
        return Ok(t);
    }
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    if let Some(t) = stored
        .as_ref()
        .and_then(|c| c.tenant.clone())
        .filter(|s| !s.is_empty())
    {
        return Ok(t);
    }
    let net = crate::networks::get(&crate::networks::effective_network_name(args, stored.as_ref()))?;
    Ok(net.tenant.domain)
}

fn parse_endpoint(s: &str) -> anyhow::Result<Url> {
    let mut s = s.to_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    Ok(Url::parse(&s)?)
}

/// Accept either separate args or a single comma-separated token list. Trims whitespace and
/// drops empty entries.
fn expand_csv(inputs: &[String]) -> Vec<String> {
    inputs
        .iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Parse `<token>:<buy|sell>` entries into `(token, Side)` pairs. Each `entries` element may
/// itself be a comma-separated list, mirroring `expand_csv` ergonomics.
fn parse_token_side_entries(
    entries: &[String],
) -> anyhow::Result<Vec<(String, predict_rs_clob_client::Side)>> {
    let mut out = Vec::new();
    for raw in entries.iter().flat_map(|s| s.split(',')) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (tok, side) = trimmed.split_once(':').ok_or_else(|| {
            anyhow!("entry '{trimmed}' must be in '<token_id>:<buy|sell>' form")
        })?;
        let side = match side.trim().to_ascii_lowercase().as_str() {
            "buy" => predict_rs_clob_client::Side::Buy,
            "sell" => predict_rs_clob_client::Side::Sell,
            other => return Err(anyhow!("invalid side '{other}' in entry '{trimmed}' — use 'buy' or 'sell'")),
        };
        out.push((tok.trim().to_owned(), side));
    }
    if out.is_empty() {
        return Err(anyhow!("at least one '<token>:<side>' entry is required"));
    }
    Ok(out)
}

#[derive(Tabled)]
struct BookRow {
    side: &'static str,
    price: String,
    size: String,
}

fn print_book(book: &OrderBookSummary, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(
            &serde_json::json!({
                "asset_id": book.asset_id,
                "timestamp": book.timestamp,
                "hash": book.hash,
                "bids": book.bids.iter().map(|l| serde_json::json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
                "asks": book.asks.iter().map(|l| serde_json::json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
            }),
        )?,
        Format::Table => {
            let mut rows: Vec<BookRow> = Vec::new();
            for lvl in book.asks.iter().rev().take(10) {
                rows.push(BookRow {
                    side: "ASK",
                    price: lvl.price.to_string(),
                    size: lvl.size.to_string(),
                });
            }
            for lvl in book.bids.iter().rev().take(10) {
                rows.push(BookRow {
                    side: "BID",
                    price: lvl.price.to_string(),
                    size: lvl.size.to_string(),
                });
            }
            if rows.is_empty() {
                println!("(empty order book for {})", book.asset_id);
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_endpoints(args: &Cli, client: &Client, fmt: Format) -> anyhow::Result<()> {
    let clob = client.clob_url().as_str().to_owned();
    let gamma = client.gamma_url().map(|u| u.as_str().to_owned());
    let ws = client.ws_url().map(|u| u.as_str().to_owned());
    let chain_id = client.chain_id();
    // Surface the resolved network selection + exchange so a user can eyeball, before placing an
    // order, exactly which network / chain / `verifyingContract` the order signer will bind to.
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let network = crate::networks::effective_network_name(args, stored.as_ref());
    let exchange = crate::networks::get(&network)
        .ok()
        .map(|n| n.contracts.ctf_exchange);
    let tenant = effective_tenant(args).ok();
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "network": network,
            "tenant": tenant,
            "clob": clob,
            "gamma": gamma,
            "ws": ws,
            "chain_id": chain_id,
            "exchange": exchange,
        }))?,
        Format::Table => {
            println!("network : {network}");
            println!("tenant  : {}", tenant.as_deref().unwrap_or("(unset)"));
            println!("clob    : {clob}");
            println!("gamma   : {}", gamma.as_deref().unwrap_or("(unset)"));
            println!("ws      : {}", ws.as_deref().unwrap_or("(unset)"));
            println!(
                "chain_id: {}",
                chain_id.map(|c| c.to_string()).unwrap_or_else(|| "(unset)".into())
            );
            println!("exchange: {}", exchange.as_deref().unwrap_or("(unset)"));
        }
    }
    Ok(())
}
