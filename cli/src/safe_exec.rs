//! Shared plumbing for chainup Safe-mode write commands (`pm approve set`, `pm ctf
//! redeem`, `pm ctf split`, `pm ctf merge`). Each command:
//!
//! 1. Resolves wallet identity (EOA + Safe + scope) from the local config.
//! 2. Builds a [`SafeTransaction`] (Call or DelegateCall+MultiSend) for its specific op.
//! 3. Reads the Safe's `nonce()` on-chain via JSON-RPC.
//! 4. Signs the SafeTx EIP-712 payload locally.
//! 5. In dry-run mode: prints the wire form and exits.
//! 6. In `--execute` mode: obtains a gamma-service JWT, POSTs to relayer-service,
//!    polls until terminal.
//!
//! All steps except #2 (the op itself) are identical across commands, so they live here.

use std::time::Duration;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use anyhow::{Context, Result, anyhow, bail};
use pm_rs_clob_client::relayer::{RelayerTransaction, SafeTxParams, SubmitRequest, SubmitType};
use pm_rs_clob_client::safe::SafeTransaction;
use pm_rs_clob_client::types::ScopeId;
use pm_rs_clob_client::{Client, Endpoints, PMCup26Signer};
use url::Url;

use crate::cli::Cli;
use crate::network_config::NetworkConfig;
use crate::output::{self, Format};

sol! {
    #[sol(rpc)]
    interface ISafe {
        function nonce() external view returns (uint256);
    }
}

/// Identity + on-chain state needed to build a Safe meta-tx for the configured wallet.
#[derive(Debug, Clone)]
pub struct SafeContext {
    pub cfg: NetworkConfig,
    pub signer: PMCup26Signer,
    pub eoa: Address,
    pub safe: Address,
    pub rpc_url: String,
}

impl SafeContext {
    /// Read the EOA private key, Safe address, scope id, and chain id from `args` + the
    /// local config and pair them with the supplied network YAML.
    pub fn resolve(args: &Cli, cfg: NetworkConfig, rpc_override: Option<&str>) -> Result<Self> {
        require_signature_type_safe(args)?;
        let safe = resolve_safe_address(args)?;
        let signer = build_signer(args, &cfg)?;
        let eoa = signer.address();
        let rpc_url = rpc_override
            .map(str::to_owned)
            .unwrap_or_else(|| cfg.network.rpc_url.clone());
        Ok(Self {
            cfg,
            signer,
            eoa,
            safe,
            rpc_url,
        })
    }

    /// Read the Safe's current `nonce()` via JSON-RPC.
    pub async fn nonce(&self) -> Result<U256> {
        read_safe_nonce(&self.rpc_url, self.safe).await
    }

    /// Sign `safe_tx` and assemble a [`SubmitRequest`] ready for the relayer.
    pub fn build_submit_request(
        &self,
        safe_tx: &SafeTransaction,
        label: &'static str,
    ) -> Result<SubmitRequest> {
        let signature = self
            .signer
            .sign_safe_tx(self.safe, safe_tx)
            .with_context(|| "sign SafeTx")?;
        let signature_hex = format!("0x{}", hex::encode(signature));

        let delegatecall = matches!(
            safe_tx.operation,
            pm_rs_clob_client::safe::SafeOperation::DelegateCall
        );
        let scope_id_hex = if self.signer.scope_id().is_zero() {
            None
        } else {
            Some(format!("{:#x}", self.signer.scope_id().as_b256()))
        };
        Ok(SubmitRequest {
            from: format!("{:#x}", self.eoa),
            to: format!("{:#x}", safe_tx.to),
            proxy_wallet: format!("{:#x}", self.safe),
            data: format!("0x{}", hex::encode(&safe_tx.data)),
            nonce: Some(safe_tx.nonce.to_string()),
            signature: signature_hex,
            signature_params: serde_json::to_value(SafeTxParams::relayer_pays(delegatecall))?,
            r#type: SubmitType::Safe,
            scope_id: scope_id_hex,
            metadata: Some(label.to_owned()),
        })
    }

    /// Build a [`pm_rs_clob_client::Client`] from the tenant YAML, log in to gamma-service,
    /// submit `req` to the relayer, and poll until terminal.
    pub async fn submit_and_poll(
        &self,
        req: &SubmitRequest,
        gamma_domain: Option<&str>,
        gamma_uri: Option<&str>,
        interval: Duration,
        timeout: Duration,
    ) -> Result<RelayerTransaction> {
        let endpoints = build_endpoints(&self.cfg)?;
        let client = Client::builder()
            .endpoints(endpoints)
            .chain_id(self.cfg.network.chain_id)
            .build()
            .context("build client")?;

        let domain = gamma_domain
            .map(str::to_owned)
            .unwrap_or_else(|| self.cfg.tenant.domain.clone());
        let uri = gamma_uri
            .map(str::to_owned)
            .unwrap_or_else(|| format!("https://{}", self.cfg.tenant.domain));

        let jwt = client
            .jwt_login(&self.signer, domain.clone(), uri.clone())
            .await
            .with_context(|| format!("jwt_login(domain={domain}, uri={uri})"))?;
        let relayer = client.relayer()?.with_token(&jwt);
        let resp = relayer.submit(req).await.with_context(|| "relayer submit")?;
        let final_tx = relayer
            .poll_until_terminal(&resp.transaction_id, interval, timeout)
            .await
            .with_context(|| format!("poll relayer tx {}", resp.transaction_id))?;
        Ok(final_tx)
    }
}

/// Render a JSON plan in table / JSON format. `dry_run = true` adds a banner reminding
/// the user nothing was submitted; `final_state` (the relayer poll result) is rendered
/// when present.
pub fn print_plan(
    plan: &serde_json::Value,
    fmt: Format,
    dry_run: bool,
    final_state: Option<serde_json::Value>,
) -> Result<()> {
    match fmt {
        Format::Json => {
            let mut full = plan.clone();
            if let Some(map) = full.as_object_mut() {
                map.insert("dry_run".into(), serde_json::Value::Bool(dry_run));
                if let Some(f) = &final_state {
                    map.insert("relayer_result".into(), f.clone());
                }
            }
            output::print_json(&full)
        }
        Format::Table => {
            let title = plan["title"].as_str().unwrap_or("safe meta-tx");
            if dry_run {
                println!("== {title} (DRY-RUN — nothing was submitted) ==");
            } else {
                println!("== {title} ==");
            }
            println!("tenant : {}", plan["tenant"].as_str().unwrap_or("?"));
            println!(
                "chain  : {} (chainId {})",
                plan["chain"]["name"].as_str().unwrap_or("?"),
                plan["chain"]["chain_id"].as_i64().unwrap_or(0)
            );
            println!("eoa    : {}", plan["wallet"]["eoa"].as_str().unwrap_or("?"));
            println!("safe   : {}", plan["wallet"]["safe"].as_str().unwrap_or("?"));
            println!(
                "op     : {} @ nonce {}",
                plan["operation"].as_str().unwrap_or("?"),
                plan["safe_nonce"].as_str().unwrap_or("?")
            );
            for (i, op) in plan["ops"].as_array().into_iter().flatten().enumerate() {
                let detail = op["detail"].as_str().unwrap_or("");
                if detail.is_empty() {
                    println!("  [{i}] {}", op["summary"].as_str().unwrap_or("?"));
                } else {
                    println!("  [{i}] {} | {detail}", op["summary"].as_str().unwrap_or("?"));
                }
            }
            println!(
                "signature: {}",
                plan["submit_request"]["signature"].as_str().unwrap_or("?")
            );
            if dry_run {
                println!("(re-run with --execute to actually submit)");
            } else if let Some(f) = &final_state {
                println!();
                println!("relayer  : id={}", f["transaction_id"].as_str().unwrap_or("?"));
                println!("           state={:?}", f["state"]);
                if let Some(h) = f["transaction_hash"].as_str()
                    && !h.is_empty()
                {
                    println!("           hash={h}");
                }
                if let Some(bn) = f["block_number"].as_u64() {
                    println!("           block={bn}");
                }
                if let Some(err) = f["error"].as_str() {
                    println!("           error={err}");
                }
            }
            Ok(())
        }
    }
}

/// Bundle every value the JSON plan needs into one helper so caller sites stay flat.
#[allow(clippy::too_many_arguments)]
pub fn assemble_plan(
    title: &str,
    ctx: &SafeContext,
    operation: &str,
    nonce: U256,
    ops: Vec<serde_json::Value>,
    req: &SubmitRequest,
) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "tenant": ctx.cfg.tenant.name,
        "chain": {
            "name": ctx.cfg.network.name,
            "chain_id": ctx.cfg.network.chain_id,
            "rpc": ctx.cfg.network.rpc_url,
        },
        "wallet": {
            "eoa": format!("{:#x}", ctx.eoa),
            "safe": format!("{:#x}", ctx.safe),
        },
        "operation": operation,
        "safe_nonce": nonce.to_string(),
        "ops": ops,
        "submit_request": req,
    })
}

/// Serialise the relayer transaction back into a flat JSON object for `print_plan`.
pub fn final_state_json(tx: &RelayerTransaction) -> serde_json::Value {
    serde_json::json!({
        "transaction_id": tx.transaction_id,
        "transaction_hash": tx.transaction_hash,
        "state": tx.state,
        "block_number": tx.block_number,
        "gas_used": tx.gas_used,
        "error": tx.error,
    })
}

// ─── primitive resolvers (also pub so caller-site validators can reuse) ─

pub fn resolve_safe_address(args: &Cli) -> Result<Address> {
    use std::str::FromStr;
    let stored = crate::config_store::load(args.config_dir.as_deref())?.ok_or_else(|| {
        anyhow!(
            "no local config — run `pm wallet create` / `pm wallet set-safe <addr>` first"
        )
    })?;
    let raw = stored.safe_address.ok_or_else(|| {
        anyhow!(
            "no Safe address configured: run `pm wallet set-safe <addr>` (manual) or `pm wallet detect-safe` (server)"
        )
    })?;
    Address::from_str(&raw).with_context(|| format!("invalid stored safe_address '{raw}'"))
}

pub fn require_signature_type_safe(args: &Cli) -> Result<()> {
    let st = crate::commands::effective_signature_type(args)?;
    if !matches!(st, pm_rs_clob_client::types::SignatureType::PolyGnosisSafe) {
        bail!(
            "Safe-mode required (signatureType=gnosis-safe); current = {st:?}. chainup only supports Safe-mode writes"
        );
    }
    Ok(())
}

pub fn build_signer(args: &Cli, cfg: &NetworkConfig) -> Result<PMCup26Signer> {
    let (pk, _source) = crate::wallet_commands::resolve_private_key(args)?;
    let mut signer = PMCup26Signer::from_hex(&pk, cfg.network.chain_id)
        .with_context(|| "invalid private key")?;
    let stored = crate::config_store::load(args.config_dir.as_deref())?;
    let scope_hex = if !args.scope_id.is_empty() {
        args.scope_id.clone()
    } else {
        stored.and_then(|c| c.scope_id).unwrap_or_default()
    };
    if !scope_hex.is_empty() {
        let scope = ScopeId::from_hex(&scope_hex)
            .with_context(|| format!("invalid scope id '{scope_hex}'"))?;
        signer = signer.with_scope_id(scope);
    }
    Ok(signer)
}

pub fn build_endpoints(cfg: &NetworkConfig) -> Result<Endpoints> {
    let clob = with_slash(&cfg.tenant.endpoints.clob);
    let mut ep = Endpoints::clob_only(&clob)
        .with_context(|| format!("invalid clob endpoint '{clob}'"))?;
    if let Some(gamma) = cfg.tenant.endpoints.gamma.as_deref() {
        let url = Url::parse(&with_slash(gamma))
            .with_context(|| format!("invalid gamma endpoint '{gamma}'"))?;
        ep = ep.with_gamma(url);
    }
    if let Some(rel) = cfg.tenant.endpoints.relayer.as_deref() {
        let url = Url::parse(&with_slash(rel))
            .with_context(|| format!("invalid relayer endpoint '{rel}'"))?;
        ep = ep.with_relayer(url);
    } else {
        bail!(
            "network config has no `tenant.endpoints.relayer` — required for Safe-mode writes"
        );
    }
    Ok(ep)
}

fn with_slash(s: &str) -> String {
    if s.ends_with('/') { s.to_owned() } else { format!("{s}/") }
}

pub async fn read_safe_nonce(rpc_url: &str, safe: Address) -> Result<U256> {
    let url = Url::parse(rpc_url).with_context(|| format!("invalid rpc_url '{rpc_url}'"))?;
    let provider = ProviderBuilder::new().connect_http(url);
    let safe_view = ISafe::new(safe, provider);
    safe_view
        .nonce()
        .call()
        .await
        .with_context(|| format!("read Safe.nonce() at {safe:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_endpoints_pulls_relayer_from_yaml_not_canonical_subdomain() {
        let cfg = crate::network_config::load("../examples/networks/monad-hermestrade.yaml")
            .expect("load monad yaml");
        let ep = build_endpoints(&cfg).unwrap();
        assert_eq!(
            ep.relayer.as_ref().unwrap().as_str(),
            "https://relayer.hermestrade.xyz/"
        );
        assert_eq!(ep.clob.as_str(), "https://clob-api.hermestrade.xyz/");
        assert_eq!(
            ep.gamma.as_ref().unwrap().as_str(),
            "https://gamma-api.hermestrade.xyz/"
        );
    }
}
