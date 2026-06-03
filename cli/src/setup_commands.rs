//! `predict-cli setup` — guided first-time configuration wizard.
//!
//! Mirrors the upstream CLI `setup`'s shape (banner → numbered steps with stdin prompts → "next
//! steps" footer) but adapted for multi-tenant, scopeId-extended, Safe-backed
//! topology. Upstream V1 has 4 steps (wallet / proxy / fund / approve); this platform has 4
//! steps too but they're different: wallet → tenant identity → Safe address → L2 API key.
//! Each step delegates the actual work to the existing subcommand or SDK call rather than
//! duplicating logic — the wizard is just a friendly entry point.

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use predict_rs_clob_client::types::ScopeId;
use predict_rs_clob_client::{Client, Credentials, PMCup26Signer};
use secrecy::ExposeSecret as _;

use crate::cli::Cli;
use crate::config_store::{self, StoredConfig};

pub async fn run(args: &Cli) -> Result<()> {
    print_banner();

    let total = 4;

    // ── 1: wallet ──────────────────────────────────────────────────────────
    step_header(1, total, "Wallet");
    let cfg = config_store::load(args.config_dir.as_deref())?;
    // Resolve in the same order every other command uses: flag/env first, config-file as
    // fallback. A key supplied via `PM_PRIVATE_KEY` should NOT be treated as "no wallet".
    let resolved_key = crate::wallet_commands::resolve_private_key(args).ok();
    let private_key = match resolved_key {
        Some((key, source)) => {
            let addr = address_for(&key)?;
            println!("  ✓ Wallet already configured ({source}).");
            println!("    EOA: {addr}");
            if !prompt_yn("  Reconfigure wallet?", false)? {
                key
            } else {
                setup_wallet(args)?
            }
        }
        None => setup_wallet(args)?,
    };
    println!();

    // ── 2: tenant + chain + scope ─────────────────────────────────────────
    step_header(2, total, "Tenant identity");
    let tenant = args
        .tenant
        .clone()
        .or_else(|| std::env::var("PM_TENANT").ok())
        .unwrap_or_default();
    let tenant = if tenant.is_empty() {
        prompt("  Tenant host (e.g. hermestrade.xyz): ")?
    } else {
        println!("  ✓ Tenant: {tenant}");
        tenant
    };

    let chain_id = match args
        .chain_id
        .or_else(|| cfg.as_ref().and_then(|c| c.chain_id))
    {
        Some(c) => {
            println!("  ✓ Chain ID: {c}");
            c
        }
        None => prompt_u64("  Chain ID [143 = Monad]: ", 143)?,
    };

    let scope_from_arg = (!args.scope_id.is_empty()).then(|| args.scope_id.clone());
    let stored_scope = cfg.as_ref().and_then(|c| c.scope_id.clone());
    let scope_id = match scope_from_arg.or(stored_scope).filter(|s| !s.is_empty()) {
        Some(s) => {
            println!("  ✓ Scope ID: {s}");
            s
        }
        None => {
            println!("  scopeId is the tenant's multi-tenant isolation key (bytes32).");
            println!("  Ask the tenant operator, or run a server call that returns it.");
            let s = prompt("  Enter scopeId (0x-prefixed bytes32) or blank to skip: ")?;
            if !s.is_empty() && !s.starts_with("0x") {
                anyhow::bail!("scopeId must start with 0x");
            }
            s
        }
    };

    let signature_type_str = pick_signature_type(args, cfg.as_ref())?;
    println!("  ✓ Signature type: {signature_type_str}");
    println!();

    // ── 3: safe address ───────────────────────────────────────────────────
    step_header(3, total, "Safe wallet");
    let safe_address = if signature_type_str == "gnosis-safe" {
        let stored_safe = cfg.as_ref().and_then(|c| c.safe_address.clone());
        match stored_safe.filter(|s| !s.is_empty()) {
            Some(s) => {
                println!("  ✓ Safe address: {s}");
                Some(s)
            }
            None => {
                println!("  In gnosis-safe mode the EOA signs but the Safe holds funds.");
                let s = prompt("  Safe address (0x…), or blank to skip and run `predict-cli wallet detect-safe` later: ")?;
                if s.is_empty() { None } else { Some(s) }
            }
        }
    } else {
        println!("  (skipped — signature type {signature_type_str} doesn't use a Safe.)");
        None
    };

    // Persist the consolidated wallet+identity config.
    let cfg_to_save = StoredConfig {
        private_key: Some(private_key.clone()),
        chain_id: Some(chain_id),
        scope_id: Some(scope_id.clone()),
        signature_type: Some(signature_type_str.clone()),
        safe_address: safe_address.clone(),
    };
    let path = config_store::save(args.config_dir.as_deref(), &cfg_to_save)?;
    println!("  ✓ Saved to {}", path.display());
    println!();

    // ── 4: L2 API key ─────────────────────────────────────────────────────
    step_header(4, total, "L2 API key");
    let creds_path = creds_path(args);
    if creds_path.exists() {
        println!("  ✓ Credentials file already present at {}", creds_path.display());
    } else if prompt_yn("  Mint a fresh L2 API key now? (POST /auth/api-key)", true)? {
        let creds = mint_api_key(&tenant, chain_id, &scope_id, &private_key, &signature_type_str)
            .await?;
        write_credentials(&creds_path, &creds)?;
        println!("  ✓ Credentials saved to {}", creds_path.display());
    } else {
        println!("  ○ Skipped. Run `predict-cli auth create-key` later when ready.");
    }
    println!();

    // ── footer ────────────────────────────────────────────────────────────
    println!("  ────────────────────────────────────");
    println!("  ✓ Setup complete!");
    println!();
    println!("  Next steps:");
    println!("    predict-cli balance --asset-type collateral");
    println!("    predict-cli approve check --network-config <yaml>");
    println!("    predict-cli gamma events list --limit 5");
    println!("    predict-cli shell");
    println!();
    Ok(())
}

// ─── helpers ──────────────────────────────────────────────────────────────

fn print_banner() {
    let bold = "\x1b[1m";
    let dim = "\x1b[2m";
    let r = "\x1b[0m";
    println!();
    println!("  {bold}predict-cli · setup wizard{r}");
    println!(
        "  {dim}Walks you through wallet, tenant, Safe address, and L2 API key.{r}"
    );
    println!();
}

fn step_header(n: u8, total: u8, label: &str) {
    println!("  [{n}/{total}] {label}");
    println!("  {}", "─".repeat(label.len() + 6));
}

fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_yn(msg: &str, default: bool) -> Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };
    let input = prompt(&format!("{msg} [{hint}] "))?;
    Ok(match input.to_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    })
}

fn prompt_u64(msg: &str, default: u64) -> Result<u64> {
    let input = prompt(msg)?;
    if input.is_empty() {
        return Ok(default);
    }
    Ok(input.parse()?)
}

fn setup_wallet(args: &Cli) -> Result<String> {
    let has_key = prompt_yn("  Do you have an existing private key?", false)?;
    if has_key {
        let key = prompt("  Enter private key (0x…): ")?;
        let _ = address_for(&key).context("invalid private key")?;
        println!("  ✓ Wallet imported");
        Ok(key)
    } else {
        // Generate a fresh key via the same path as `predict-cli wallet create`.
        let key = generate_random_key();
        let addr = address_for(&key)?;
        println!("  ✓ Wallet created");
        println!("    EOA: {addr}");
        println!();
        println!("  ⚠ Back up the private key from the config file.");
        println!("    Path: {}", config_store::config_path(args.config_dir.as_deref())?.display());
        Ok(key)
    }
}

fn pick_signature_type(args: &Cli, cfg: Option<&StoredConfig>) -> Result<String> {
    if let Some(s) = args.signature_type {
        return Ok(label_for(s));
    }
    if let Some(s) = cfg.and_then(|c| c.signature_type.clone()) {
        return Ok(s);
    }
    let input = prompt("  Signature type [gnosis-safe / eoa / proxy] (default gnosis-safe): ")?;
    Ok(match input.as_str() {
        "" => "gnosis-safe".into(),
        s @ ("gnosis-safe" | "eoa" | "proxy") => s.into(),
        other => anyhow::bail!("unrecognised signature type {other:?}"),
    })
}

fn label_for(s: crate::cli::SignatureTypeArg) -> String {
    use crate::cli::SignatureTypeArg::*;
    match s {
        Eoa => "eoa".into(),
        Proxy => "proxy".into(),
        GnosisSafe => "gnosis-safe".into(),
    }
}

fn address_for(key: &str) -> Result<String> {
    let signer = PMCup26Signer::from_hex(key, 0).context("invalid private key")?;
    Ok(format!("{:#x}", signer.address()))
}

fn generate_random_key() -> String {
    // Reuse the same primitive `predict-cli wallet create` uses (alloy's k256 RNG) so a key
    // minted here is interchangeable with one minted via the wallet subcommand.
    let signer = alloy::signers::local::PrivateKeySigner::random();
    format!("0x{}", hex::encode(signer.to_bytes()))
}

async fn mint_api_key(
    tenant: &str,
    chain_id: u64,
    scope_id: &str,
    private_key: &str,
    _signature_type_str: &str,
) -> Result<Credentials> {
    // L1 `ClobAuth` signing doesn't depend on signature_type — that's only an order-level
    // concern. The signer just needs chain_id + scope_id.
    let signer = PMCup26Signer::from_hex(private_key, chain_id)?;
    let signer = if scope_id.is_empty() {
        signer
    } else {
        signer.with_scope_id(ScopeId::from_hex(scope_id.trim_start_matches("0x"))?)
    };
    let client = Client::builder()
        .tenant(tenant)?
        .chain_id(chain_id)
        .build()?;
    client.create_api_key(&signer, None).await.map_err(Into::into)
}

fn creds_path(args: &Cli) -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PM_CREDENTIALS_FILE") {
        return std::path::PathBuf::from(p);
    }
    let dir = config_store::config_dir(args.config_dir.as_deref())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    dir.join("credentials.json")
}

fn write_credentials(path: &std::path::Path, creds: &Credentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::json!({
        "apiKey": creds.key.to_string(),
        "secret": creds.secret.expose_secret(),
        "passphrase": creds.passphrase.expose_secret(),
    });
    let raw = serde_json::to_string_pretty(&body)?;
    std::fs::write(path, raw)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
