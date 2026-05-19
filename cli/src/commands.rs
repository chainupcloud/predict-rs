//! Command dispatch.

use anyhow::{Context, anyhow};
use chrono::DateTime;
use pm_rs_clob_client::clob::types::OrderBookSummary;
use pm_rs_clob_client::{Client, Endpoints};
use tabled::Tabled;
use url::Url;

use crate::cli::{Cli, Command};
use crate::output::{self, Format};

pub async fn run(args: Cli) -> anyhow::Result<()> {
    let mut builder = Client::builder();

    let endpoints = resolve_endpoints(&args)?;
    builder = builder.endpoints(endpoints);
    if let Some(cid) = args.chain_id {
        builder = builder.chain_id(cid);
    }

    let client = builder.build().context("build client")?;
    let fmt = args.output;

    match args.command {
        Command::Endpoints => {
            print_endpoints(&client, fmt)?;
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
        Command::Gamma(a) => {
            crate::gamma_commands::run(client, fmt, a).await?;
        }
    }
    Ok(())
}

fn resolve_endpoints(args: &Cli) -> anyhow::Result<Endpoints> {
    match (&args.tenant, &args.clob_endpoint) {
        (Some(host), _) => {
            let mut ep = Endpoints::from_tenant(host)
                .with_context(|| format!("derive endpoints from tenant {host}"))?;
            if let Some(g) = &args.gamma_endpoint {
                ep = ep.with_gamma(parse_endpoint(g)?);
            }
            if let Some(w) = &args.ws_endpoint {
                ep = ep.with_ws(parse_endpoint(w)?);
            }
            Ok(ep)
        }
        (None, Some(clob)) => {
            let mut ep = Endpoints::clob_only(clob)
                .with_context(|| format!("parse clob endpoint {clob}"))?;
            if let Some(g) = &args.gamma_endpoint {
                ep = ep.with_gamma(parse_endpoint(g)?);
            }
            if let Some(w) = &args.ws_endpoint {
                ep = ep.with_ws(parse_endpoint(w)?);
            }
            Ok(ep)
        }
        (None, None) => Err(anyhow!(
            "no endpoints configured: pass --tenant <host> or --clob-endpoint <url> \
             (or set PM_TENANT / PM_CLOB_ENDPOINT env vars)"
        )),
    }
}

fn parse_endpoint(s: &str) -> anyhow::Result<Url> {
    let mut s = s.to_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    Ok(Url::parse(&s)?)
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

fn print_endpoints(client: &Client, fmt: Format) -> anyhow::Result<()> {
    let clob = client.clob_url().as_str().to_owned();
    let gamma = client.gamma_url().map(|u| u.as_str().to_owned());
    let ws = client.ws_url().map(|u| u.as_str().to_owned());
    let chain_id = client.chain_id();
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "clob": clob,
            "gamma": gamma,
            "ws": ws,
            "chain_id": chain_id,
        }))?,
        Format::Table => {
            println!("clob    : {clob}");
            println!("gamma   : {}", gamma.as_deref().unwrap_or("(unset)"));
            println!("ws      : {}", ws.as_deref().unwrap_or("(unset)"));
            println!(
                "chain_id: {}",
                chain_id.map(|c| c.to_string()).unwrap_or_else(|| "(unset)".into())
            );
        }
    }
    Ok(())
}
