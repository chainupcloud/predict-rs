//! Command dispatch.

use anyhow::Context;
use chrono::DateTime;
use pm_rs_clob_client::Client;
use pm_rs_clob_client::clob::types::OrderBookSummary;
use tabled::Tabled;

use crate::cli::{Cli, Command};
use crate::output::{self, Format};

pub async fn run(args: Cli) -> anyhow::Result<()> {
    let client = Client::builder()
        .endpoint(&args.endpoint)
        .build()
        .with_context(|| format!("build client for endpoint {}", args.endpoint))?;

    let fmt = args.output;

    match args.command {
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
    }
    Ok(())
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
            // Show top 10 asks (descending price), then top 10 bids (descending).
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
