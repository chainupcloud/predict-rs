//! `predict-cli wallet deploy-safe` — create the EOA's Gnosis Safe via the relayer's `SAFE-CREATE`.
//!
//! Unlike every other on-chain write, there is no existing Safe to route through: the EOA signs an
//! EIP-712 `CreateProxy` digest, the relayer reconstructs `SafeProxyFactory.createProxy(...)`
//! calldata and pays gas to CREATE2-deploy a deterministic Safe at `computeProxyAddress(eoa,
//! scopeId)`. One-shot per `(eoa, scopeId)` — a duplicate is rejected. The `data` field is ignored
//! by the relayer for SAFE-CREATE; it rebuilds calldata from `signatureParams` + `signature`.

use std::time::Duration;

use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use alloy::sol_types::{SolStruct, eip712_domain};
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use predict_rs_clob_client::relayer::{SafeCreateParams, SubmitRequest, SubmitType};
use predict_rs_clob_client::Client;

use crate::cli::Cli;
use crate::output::{self, Format};

sol! {
    // EIP-712 typed struct the EOA signs (SafeProxyFactory CREATE_PROXY_TYPEHASH).
    struct CreateProxy {
        address paymentToken;
        uint256 payment;
        address paymentReceiver;
        bytes32 scopeId;
    }

    #[sol(rpc)]
    interface ISafeProxyFactory {
        function computeProxyAddress(address user, bytes32 scopeId) external view returns (address);
    }
}

#[derive(Debug, Args)]
pub struct DeploySafeArgs {
    /// Predict + sign but do NOT submit to the relayer.
    #[arg(long)]
    pub dry_run: bool,
    /// On success, save the deployed Safe to `config.toml` `safe_address`.
    #[arg(long, default_value_t = true)]
    pub save: bool,
    /// Override the network RPC URL.
    #[arg(long)]
    pub rpc_url: Option<String>,
    #[arg(long, default_value_t = 2)]
    pub poll_interval_secs: u64,
    #[arg(long, default_value_t = 120)]
    pub poll_timeout_secs: u64,
}

pub async fn run(args: &Cli, a: &DeploySafeArgs, fmt: Format) -> Result<()> {
    let net = crate::networks::effective_network(args)?;
    let chain_id = net.network.chain_id;
    let rpc = a.rpc_url.clone().unwrap_or_else(|| net.network.rpc_url.clone());
    let factory = parse_addr(
        net.contracts
            .safe_proxy_factory
            .as_deref()
            .or_else(|| net.security.as_ref().and_then(|s| s.safe_proxy_factory.as_deref()))
            .ok_or_else(|| anyhow!("network '{}' has no safe_proxy_factory", net.network.name))?,
    )?;

    // EOA signer (key + chain + scope) — reuse the shared resolver.
    let signer = crate::commands::signer_from_args(args)?;
    let eoa = signer.address();
    let scope = signer.scope_id();
    if scope.is_zero() {
        bail!("SAFE-CREATE requires a scope_id — pass --scope-id or set it in config.toml");
    }
    let scope_b256 = scope.as_b256();
    let scope_hex = format!("{scope_b256:#x}");

    // Predict the deterministic Safe address via the factory view (must equal the relayer's
    // own CREATE2 prediction, else the relayer rejects).
    let provider = ProviderBuilder::new().connect_http(rpc.parse().with_context(|| format!("rpc url {rpc}"))?);
    let predicted = ISafeProxyFactory::new(factory, &provider)
        .computeProxyAddress(eoa, scope_b256)
        .call()
        .await
        .context("computeProxyAddress")?;

    // EIP-712 CreateProxy digest. NOTE: the factory's domain has NO `version` field — omit it.
    let domain = eip712_domain! {
        name: "Polymarket Contract Proxy Factory",
        chain_id: chain_id,
        verifying_contract: factory,
    };
    let create = CreateProxy {
        paymentToken: Address::ZERO,
        payment: U256::ZERO,
        paymentReceiver: Address::ZERO,
        scopeId: scope_b256,
    };
    let digest = create.eip712_signing_hash(&domain);
    let sig = signer.sign_digest(digest).map_err(|e| anyhow!("sign CreateProxy: {e}"))?;
    let mut sig_bytes = [0u8; 65];
    sig_bytes[0..32].copy_from_slice(&sig.r().to_be_bytes::<32>());
    sig_bytes[32..64].copy_from_slice(&sig.s().to_be_bytes::<32>());
    sig_bytes[64] = 27 + sig.v() as u8; // ethereum-style v {27,28} (relayer also accepts {0,1})

    let zero = format!("{:#x}", Address::ZERO);
    let req = SubmitRequest {
        from: format!("{eoa:#x}"),
        to: format!("{factory:#x}"),
        proxy_wallet: format!("{predicted:#x}"),
        data: "0x".into(),
        nonce: None,
        signature: format!("0x{}", hex::encode(sig_bytes)),
        signature_params: serde_json::to_value(SafeCreateParams {
            payment_token: zero.clone(),
            payment: "0".into(),
            payment_receiver: zero,
            scope_id: scope_hex.clone(),
        })?,
        r#type: SubmitType::SafeCreate,
        scope_id: Some(scope_hex.clone()),
        metadata: Some("safe-create".into()),
    };

    if matches!(fmt, Format::Json) {
        output::print_json(&serde_json::json!({
            "action": "deploy-safe (SAFE-CREATE)",
            "eoa": format!("{eoa:#x}"),
            "factory": format!("{factory:#x}"),
            "scope_id": scope_hex,
            "predicted_safe": format!("{predicted:#x}"),
            "dry_run": a.dry_run,
        }))?;
    } else {
        println!("deploy-safe (SAFE-CREATE via relayer):");
        println!("  eoa            : {eoa:#x}");
        println!("  factory        : {factory:#x}");
        println!("  scope_id       : {scope_hex}");
        println!("  predicted safe : {predicted:#x}");
    }

    if a.dry_run {
        return Ok(());
    }

    // Already deployed? (one-shot per (eoa, scopeId).)
    if provider.get_code_at(predicted).await.map(|c| !c.is_empty()).unwrap_or(false) {
        bail!("a Safe is already deployed at {predicted:#x} for this (EOA, scopeId)");
    }

    // Submit via the relayer. No existing Safe → replicate the client + jwt + submit path
    // standalone (SafeContext can't be used here, it requires a configured Safe).
    let endpoints = crate::safe_exec::build_endpoints(&net)?;
    let client = Client::builder()
        .endpoints(endpoints)
        .chain_id(chain_id)
        .build()
        .context("build client")?;
    let uri = format!("https://{}", net.tenant.domain);
    let jwt = client
        .jwt_login(&signer, net.tenant.domain.clone(), uri)
        .await
        .with_context(|| format!("jwt_login({})", net.tenant.domain))?;
    let relayer = client.relayer()?.with_token(&jwt);
    let resp = relayer.submit(&req).await.context("relayer submit (SAFE-CREATE)")?;
    let _final = relayer
        .poll_until_terminal(
            &resp.transaction_id,
            Duration::from_secs(a.poll_interval_secs.max(1)),
            Duration::from_secs(a.poll_timeout_secs.max(5)),
        )
        .await
        .with_context(|| format!("poll relayer tx {}", resp.transaction_id))?;

    let deployed = provider.get_code_at(predicted).await.map(|c| !c.is_empty()).unwrap_or(false);
    if deployed && a.save {
        let mut cfg = crate::config_store::load(args.config_dir.as_deref())?.unwrap_or_default();
        cfg.safe_address = Some(format!("{predicted:#x}"));
        crate::config_store::save(args.config_dir.as_deref(), &cfg)?;
    }
    if matches!(fmt, Format::Json) {
        output::print_json(&serde_json::json!({
            "safe": format!("{predicted:#x}"),
            "deployed": deployed,
            "saved_to_config": deployed && a.save,
        }))?;
    } else {
        println!(
            "  result         : {} {predicted:#x}{}",
            if deployed { "deployed ✓" } else { "NO CODE (check relayer tx)" },
            if deployed && a.save { " — saved to config" } else { "" },
        );
    }
    if !deployed {
        bail!("relayer finished but no code at {predicted:#x}");
    }
    Ok(())
}

fn parse_addr(s: &str) -> Result<Address> {
    s.trim().parse::<Address>().map_err(|e| anyhow!("invalid address '{s}': {e}"))
}
