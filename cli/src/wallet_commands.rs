//! `pm wallet` subcommands — local-only key + config-file management.
//!
//! Mirrors `polymarket wallet create / import / address / show / reset`. All actions touch
//! `<config-dir>/config.toml` only; no network calls.

use std::io::{BufRead, Write as _};

use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};

use crate::cli::Cli;
use crate::config_store;
use crate::output::{self, Format};

#[derive(Debug, Subcommand)]
pub enum WalletCommand {
    /// Generate a random secp256k1 key and write it to `<config-dir>/config.toml`.
    Create(CreateArgs),
    /// Persist an existing hex-encoded private key.
    Import(ImportArgs),
    /// Print the EOA address resolved from the active key source (flag / env / config file).
    Address,
    /// Print the active key's address plus where it was loaded from.
    Show,
    /// Delete `<config-dir>/config.toml`. Prompts unless `--force`.
    Reset(ResetArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Overwrite an existing config without prompting.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Hex-encoded private key (`0x...` or bare 32-byte hex).
    pub key: String,
    /// Overwrite an existing config without prompting.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ResetArgs {
    /// Skip the y/N confirmation.
    #[arg(long)]
    pub force: bool,
}

pub async fn run(args: &Cli, sub: &WalletCommand, fmt: Format) -> Result<()> {
    let dir_override = args.config_dir.as_deref();
    match sub {
        WalletCommand::Create(a) => run_create(dir_override, a.force, fmt),
        WalletCommand::Import(a) => run_import(dir_override, &a.key, a.force, fmt),
        WalletCommand::Address => run_address(args, fmt),
        WalletCommand::Show => run_show(args, fmt),
        WalletCommand::Reset(a) => run_reset(dir_override, a.force, fmt),
    }
}

fn run_create(dir_override: Option<&str>, force: bool, fmt: Format) -> Result<()> {
    refuse_overwrite_unless_forced(dir_override, force)?;
    let signer = PrivateKeySigner::random();
    let pk_hex = format!("0x{}", hex::encode(signer.to_bytes()));
    let mut cfg = config_store::load(dir_override)?.unwrap_or_default();
    cfg.private_key = Some(pk_hex);
    let path = config_store::save(dir_override, &cfg)?;
    let address = format!("{:?}", signer.address());
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "address": address,
            "path": path.display().to_string(),
        }))?,
        Format::Table => {
            println!("Generated new wallet");
            println!("address: {address}");
            println!("saved  : {}", path.display());
        }
    }
    Ok(())
}

fn run_import(dir_override: Option<&str>, raw: &str, force: bool, fmt: Format) -> Result<()> {
    let signer = parse_signer(raw)?;
    refuse_overwrite_unless_forced(dir_override, force)?;
    let pk_hex = normalize_hex(raw);
    let mut cfg = config_store::load(dir_override)?.unwrap_or_default();
    cfg.private_key = Some(pk_hex);
    let path = config_store::save(dir_override, &cfg)?;
    let address = format!("{:?}", signer.address());
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "address": address,
            "path": path.display().to_string(),
        }))?,
        Format::Table => {
            println!("Imported wallet");
            println!("address: {address}");
            println!("saved  : {}", path.display());
        }
    }
    Ok(())
}

fn run_address(args: &Cli, fmt: Format) -> Result<()> {
    let (pk, _source) = resolve_private_key(args)?;
    let signer = parse_signer(&pk)?;
    let address = format!("{:?}", signer.address());
    output::print_scalar("address", address, fmt)
}

fn run_show(args: &Cli, fmt: Format) -> Result<()> {
    let dir_override = args.config_dir.as_deref();
    let path = config_store::config_path(dir_override)?;
    match resolve_private_key(args) {
        Ok((pk, source)) => {
            let signer = parse_signer(&pk)?;
            let address = format!("{:?}", signer.address());
            match fmt {
                Format::Json => output::print_json(&serde_json::json!({
                    "address": address,
                    "source": source,
                    "config_path": path.display().to_string(),
                }))?,
                Format::Table => {
                    println!("address    : {address}");
                    println!("source     : {source}");
                    println!("config path: {}", path.display());
                }
            }
            Ok(())
        }
        Err(_) => match fmt {
            Format::Json => output::print_json(&serde_json::json!({
                "address": serde_json::Value::Null,
                "source": "none",
                "config_path": path.display().to_string(),
            })),
            Format::Table => {
                println!("address    : (none configured)");
                println!("source     : none");
                println!("config path: {}", path.display());
                Ok(())
            }
        },
    }
}

fn run_reset(dir_override: Option<&str>, force: bool, fmt: Format) -> Result<()> {
    let path = config_store::config_path(dir_override)?;
    if !path.exists() {
        match fmt {
            Format::Json => output::print_json(&serde_json::json!({
                "removed": false,
                "path": path.display().to_string(),
            }))?,
            Format::Table => println!("nothing to reset (no config at {})", path.display()),
        }
        return Ok(());
    }
    if !force {
        let answer = prompt(&format!(
            "Delete {} and forget the stored wallet? [y/N] ",
            path.display()
        ))?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            bail!("aborted");
        }
    }
    let removed = config_store::delete(dir_override)?;
    match fmt {
        Format::Json => output::print_json(&serde_json::json!({
            "removed": removed,
            "path": path.display().to_string(),
        }))?,
        Format::Table => {
            if removed {
                println!("removed {}", path.display());
            } else {
                println!("nothing to reset (no config at {})", path.display());
            }
        }
    }
    Ok(())
}

fn refuse_overwrite_unless_forced(dir_override: Option<&str>, force: bool) -> Result<()> {
    if force {
        return Ok(());
    }
    if let Some(cfg) = config_store::load(dir_override)?
        && cfg.private_key.is_some()
    {
        let path = config_store::config_path(dir_override)?;
        bail!(
            "config at {} already has a private key; pass --force to overwrite",
            path.display()
        );
    }
    Ok(())
}

fn parse_signer(hex_str: &str) -> Result<PrivateKeySigner> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(stripped).context("decode private-key hex")?;
    if bytes.len() != 32 {
        bail!("private key must be 32 bytes, got {}", bytes.len());
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&bytes);
    PrivateKeySigner::from_bytes(&buf.into())
        .map_err(|e| anyhow!("invalid private key: {e}"))
}

fn normalize_hex(s: &str) -> String {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    format!("0x{}", stripped.to_ascii_lowercase())
}

/// Resolution order: `--private-key` / `PM_PRIVATE_KEY` (clap merges them) → config file.
/// Returns the hex string and a human-readable source label.
pub(crate) fn resolve_private_key(args: &Cli) -> Result<(String, String)> {
    if let Some(pk) = args.private_key.as_deref() {
        return Ok((pk.to_owned(), "cli (--private-key / PM_PRIVATE_KEY)".into()));
    }
    let path = config_store::config_path(args.config_dir.as_deref())?;
    let cfg = config_store::load(args.config_dir.as_deref())?
        .ok_or_else(|| anyhow!("no private key configured: pass --private-key, set PM_PRIVATE_KEY, or run `pm wallet create`"))?;
    let pk = cfg.private_key.ok_or_else(|| {
        anyhow!(
            "config file {} has no `private_key` entry; run `pm wallet create` or `pm wallet import`",
            path.display()
        )
    })?;
    Ok((pk, format!("config-file {}", path.display())))
}

fn prompt(msg: &str) -> Result<String> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_then_address_roundtrip() {
        let t = TempDir::new().unwrap();
        let dir = t.path().to_string_lossy().into_owned();
        run_create(Some(&dir), false, Format::Json).unwrap();

        let cfg = config_store::load(Some(&dir)).unwrap().unwrap();
        let pk = cfg.private_key.expect("private key stored");
        let signer = parse_signer(&pk).unwrap();
        let addr = format!("{:?}", signer.address());
        assert!(addr.starts_with("0x"));
    }

    #[test]
    fn create_refuses_overwrite_without_force() {
        let t = TempDir::new().unwrap();
        let dir = t.path().to_string_lossy().into_owned();
        run_create(Some(&dir), false, Format::Json).unwrap();
        let err = run_create(Some(&dir), false, Format::Json).unwrap_err();
        assert!(format!("{err}").contains("already has a private key"));
    }

    #[test]
    fn import_persists_key() {
        let t = TempDir::new().unwrap();
        let dir = t.path().to_string_lossy().into_owned();
        let key = "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";
        run_import(Some(&dir), key, false, Format::Json).unwrap();
        let cfg = config_store::load(Some(&dir)).unwrap().unwrap();
        assert_eq!(cfg.private_key.as_deref(), Some(key));
    }

    #[test]
    fn reset_force_deletes() {
        let t = TempDir::new().unwrap();
        let dir = t.path().to_string_lossy().into_owned();
        run_create(Some(&dir), false, Format::Json).unwrap();
        run_reset(Some(&dir), true, Format::Json).unwrap();
        assert!(config_store::load(Some(&dir)).unwrap().is_none());
    }
}
