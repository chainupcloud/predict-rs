//! Output formatting — `table` (default, human-readable) or `json` (machine-readable).

use clap::ValueEnum;
use serde::Serialize;
use tabled::{Table, Tabled, settings::Style};

#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Table,
    Json,
}

pub fn print_json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    serde_json::to_writer_pretty(std::io::stdout(), value)?;
    println!();
    Ok(())
}

pub fn print_table<T>(rows: impl IntoIterator<Item = T>) where T: Tabled {
    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");
}

pub fn print_scalar(label: &str, value: impl std::fmt::Display, format: Format) -> anyhow::Result<()> {
    match format {
        Format::Json => {
            print_json(&serde_json::json!({ label: value.to_string() }))?;
        }
        Format::Table => {
            println!("{label}: {value}");
        }
    }
    Ok(())
}
