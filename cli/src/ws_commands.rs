//! CLI subcommands for the CLOB WebSocket channels.
//!
//! Wired into [`crate::commands::run`] via a single `Command::Ws` match arm so
//! the diff with shared CLI files stays minimal.

use std::time::Duration;

use anyhow::{Context as _, anyhow};
use futures::StreamExt as _;
use predict_rs_clob_client::clob::ws::types::request::MarketLevel;
use predict_rs_clob_client::{
    Client, ClientBuilder, ClobWebSocketClient, Credentials, MarketSubscribeOpts,
    PMCup26Signer,
};
use predict_rs_clob_client::clob::ws::types::response::{MarketEvent, UserEvent};

use crate::cli::{Cli, WsArgs, WsBookArgs, WsBookWatchArgs, WsCmd, WsUserArgs};
use crate::output::{self, Format};

pub async fn run(args: Cli, fmt: Format, wargs: WsArgs) -> anyhow::Result<()> {
    match wargs.command {
        WsCmd::Ping => run_ping(&args).await,
        WsCmd::Book(a) => run_book(&args, a, fmt).await,
        WsCmd::BookWatch(a) => run_book_watch(&args, a).await,
        WsCmd::User(a) => run_user(&args, a, fmt).await,
    }
}

async fn run_ping(args: &Cli) -> anyhow::Result<()> {
    let client = build_unauth_client(args)?;
    let ws = client.clob_ws().context("clob_ws")?;
    ws.ping(Duration::from_secs(10)).await?;
    println!("ok");
    Ok(())
}

async fn run_book(args: &Cli, a: WsBookArgs, fmt: Format) -> anyhow::Result<()> {
    let client = build_unauth_client(args)?;
    let ws = client.clob_ws().context("clob_ws")?;
    let mut opts = MarketSubscribeOpts::default();
    if a.no_initial_dump {
        opts = opts.with_initial_dump(false);
    }
    if let Some(level) = a.level {
        opts = opts.with_level(MarketLevel::try_from(level).map_err(|e| anyhow!(e))?);
    }
    if a.custom_features {
        opts = opts.with_custom_features(true);
    }
    let mut stream = ws.subscribe_market(a.asset_ids, opts).await?;
    let mut received = 0u32;
    let cancel = ctrl_c_signal();
    tokio::pin!(cancel);
    while received < a.count {
        tokio::select! {
            biased;
            _ = &mut cancel => break,
            next = stream.next() => match next {
                Some(Ok(ev)) => {
                    print_market_event(&ev, fmt)?;
                    received += 1;
                }
                Some(Err(e)) => return Err(anyhow!("ws error: {e}")),
                None => break,
            }
        }
    }
    Ok(())
}

async fn run_book_watch(args: &Cli, a: WsBookWatchArgs) -> anyhow::Result<()> {
    let client = build_unauth_client(args)?;
    let ws = client.clob_ws().context("clob_ws")?;
    let mut stream = ws
        .subscribe_market(vec![a.asset_id.clone()], MarketSubscribeOpts::default())
        .await?;
    let cancel = ctrl_c_signal();
    tokio::pin!(cancel);

    let asset_id = a.asset_id;
    let json_mode = a.print_as_json && !a.print_as_table;
    loop {
        tokio::select! {
            biased;
            _ = &mut cancel => break,
            next = stream.next() => match next {
                Some(Ok(ev)) => print_watch_line(&asset_id, &ev, json_mode)?,
                Some(Err(e)) => return Err(anyhow!("ws error: {e}")),
                None => break,
            }
        }
    }
    Ok(())
}

async fn run_user(args: &Cli, a: WsUserArgs, fmt: Format) -> anyhow::Result<()> {
    let client = build_l2_client(args).await?;
    let ws = client.clob_ws().context("clob_ws")?;
    let mut stream = ws.subscribe_user(a.markets).await?;
    let cancel = ctrl_c_signal();
    tokio::pin!(cancel);
    loop {
        tokio::select! {
            biased;
            _ = &mut cancel => break,
            next = stream.next() => match next {
                Some(Ok(ev)) => print_user_event(&ev, fmt)?,
                Some(Err(e)) => return Err(anyhow!("ws error: {e}")),
                None => break,
            }
        }
    }
    Ok(())
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn build_unauth_client(args: &Cli) -> anyhow::Result<Client> {
    let endpoints = crate::commands::resolve_endpoints_pub(args)?;
    let mut b = Client::builder().endpoints(endpoints);
    if let Some(cid) = args.chain_id {
        b = b.chain_id(cid);
    }
    b.build().context("build client")
}

async fn build_l2_client(args: &Cli) -> anyhow::Result<Client> {
    use predict_rs_clob_client::types::ScopeId;
    let pk = args.private_key.as_deref().ok_or_else(|| {
        anyhow!("private key required for /ws/user: pass --private-key or set PM_PRIVATE_KEY")
    })?;
    let chain_id = args.chain_id.ok_or_else(|| {
        anyhow!("chain id required for /ws/user: pass --chain-id or set PM_CHAIN_ID")
    })?;
    let mut signer = PMCup26Signer::from_hex(pk, chain_id)?;
    if !args.scope_id.is_empty() {
        let scope = ScopeId::from_hex(&args.scope_id)
            .with_context(|| format!("invalid --scope-id '{}'", args.scope_id))?;
        signer = signer.with_scope_id(scope);
    }

    let creds = match args.credentials.as_deref() {
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("read --credentials file {path}"))?;
            serde_json::from_str::<Credentials>(&raw)
                .with_context(|| format!("decode credentials file {path}"))?
        }
        None => {
            let bootstrap = build_unauth_client(args)?;
            bootstrap.create_or_derive_api_key(&signer, None).await?
        }
    };

    let endpoints = crate::commands::resolve_endpoints_pub(args)?;
    let mut b: ClientBuilder = Client::builder().endpoints(endpoints);
    if let Some(cid) = args.chain_id {
        b = b.chain_id(cid);
    }
    Ok(b.credentials(creds).signer_address(signer.address()).build()?)
}

async fn ctrl_c_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn print_market_event(ev: &MarketEvent, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&serde_json::to_value(ev)?)?,
        Format::Table => println!("{}", serde_json::to_string(ev)?),
    }
    Ok(())
}

fn print_user_event(ev: &UserEvent, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&serde_json::to_value(ev)?)?,
        Format::Table => println!("{}", serde_json::to_string(ev)?),
    }
    Ok(())
}

fn print_watch_line(asset_id: &str, ev: &MarketEvent, json_mode: bool) -> anyhow::Result<()> {
    if json_mode {
        println!("{}", serde_json::to_string(ev)?);
        return Ok(());
    }
    match ev {
        MarketEvent::Book(b) if b.asset_id == asset_id => {
            let best_bid = b.bids.iter().next_back().map_or("-".into(), |l| l.price.clone());
            let best_ask = b.asks.first().map_or("-".into(), |l| l.price.clone());
            println!("[BOOK ] asset={} bid={} ask={}", asset_id, best_bid, best_ask);
        }
        MarketEvent::PriceChange(p) => {
            for ch in &p.price_changes {
                if ch.asset_id != asset_id {
                    continue;
                }
                println!(
                    "[DELTA] asset={} side={:?} price={} size={} best_bid={} best_ask={}",
                    asset_id, ch.side, ch.price, ch.size, ch.best_bid, ch.best_ask,
                );
            }
        }
        MarketEvent::LastTradePrice(lt) if lt.asset_id == asset_id => {
            println!(
                "[TRADE] asset={} side={:?} price={} size={}",
                asset_id, lt.side, lt.price, lt.size,
            );
        }
        MarketEvent::BestBidAsk(bba) if bba.asset_id == asset_id => {
            println!(
                "[BBA  ] asset={} bid={} ask={} spread={}",
                asset_id, bba.best_bid, bba.best_ask, bba.spread,
            );
        }
        _ => {}
    }
    Ok(())
}

// We expose ClobWebSocketClient so callers can construct directly outside the CLI.
#[allow(dead_code)]
fn _public_surface_hint(_c: ClobWebSocketClient) {}
