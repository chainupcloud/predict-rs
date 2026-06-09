//! Persistent CLI configuration on disk.
//!
//! The store backs `predict-cli wallet` and is read as a fallback by every command that needs a
//! private key / chain id / scope id when those values are not supplied via flag or env.
//!
//! Resolution for the config directory:
//!   1. `--config-dir <path>` flag
//!   2. `PM_CONFIG_DIR` env var
//!   3. `dirs::config_dir()/predict` (Linux: `~/.config/predict`, macOS: `~/Library/Application Support/predict`)
//!
//! An optional `--slug <name>` / `PM_SLUG` nests one level deeper — the effective directory
//! becomes `<base>/<name>` — so several accounts can share one base. See [`resolve_with_slug`].
//!
//! The store is a single TOML file `config.toml` inside that directory. Writes are atomic
//! (write to a sibling temp file then rename) and the file is created with mode 0600 on
//! Unix; the parent directory is created with mode 0700.

use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "config.toml";

/// The on-disk schema. All fields are optional so a partial config (e.g. wallet without a
/// chain id) is representable; downstream commands keep their existing required-arg checks.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConfig {
    /// Hex-encoded secp256k1 private key (with or without `0x` prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    /// `bytes32` hex string (with `0x` prefix). Empty string is treated as "no scope".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    /// One of `eoa` / `proxy` / `gnosis-safe` (matches the `SignatureTypeArg` clap value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_type: Option<String>,
    /// Safe wallet (`bytes20` hex). Required by default `signatureType=gnosis-safe`
    /// flows since the Safe — not the EOA — holds USDC / CTF balances. Populate via
    /// `predict-cli wallet set-safe <addr>` (manual) or `predict-cli wallet detect-safe` (one server call).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_address: Option<String>,
    /// Active built-in network name (see [`crate::networks`]). Selects chain id, endpoints, and
    /// contract addresses. Overridden by `--network`; defaults to [`crate::networks::DEFAULT_NETWORK`]
    /// when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Tenant root host override. Most deployments leave this unset and inherit the selected
    /// network's domain; set it for the same-network / different-tenant case. Overridden by
    /// `--tenant`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

/// Resolve the directory holding `config.toml`, honoring `--config-dir` then `PM_CONFIG_DIR`
/// then the platform default.
pub fn config_dir(cli_override: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = cli_override {
        return Ok(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("PM_CONFIG_DIR") {
        return Ok(PathBuf::from(p));
    }
    let base = dirs::config_dir()
        .ok_or_else(|| anyhow!("could not determine OS config directory; pass --config-dir"))?;
    Ok(base.join("predict"))
}

pub fn config_path(cli_override: Option<&str>) -> Result<PathBuf> {
    Ok(config_dir(cli_override)?.join(CONFIG_FILE_NAME))
}

/// Resolve the config dir, applying an optional account `slug` as a nested leaf under the base
/// (`<base>/<slug>`). The base is resolved by [`config_dir`], so `--config-dir` / `PM_CONFIG_DIR`
/// / the platform default all act as the base. A `None` slug yields the base unchanged.
pub fn resolve_with_slug(cli_override: Option<&str>, slug: Option<&str>) -> Result<PathBuf> {
    let base = config_dir(cli_override)?;
    match slug {
        Some(s) => {
            validate_slug(s)?;
            Ok(base.join(s))
        }
        None => Ok(base),
    }
}

/// A slug must be a single, normal path segment so it can only ever name a direct child of the
/// base dir — never escape it. Rejects empty, separators, `.` / `..`, and absolute fragments.
fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        return Err(anyhow!("--slug must not be empty"));
    }
    let mut comps = Path::new(slug).components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(anyhow!(
            "invalid --slug {slug:?}: must be a single path segment (no '/', '\\', '..', '.')"
        )),
    }
}

/// Returns `Ok(None)` if the config file does not exist, otherwise the decoded `StoredConfig`.
pub fn load(cli_override: Option<&str>) -> Result<Option<StoredConfig>> {
    let path = config_path(cli_override)?;
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let cfg: StoredConfig = toml::from_str(&raw)
                .with_context(|| format!("decode config file {}", path.display()))?;
            Ok(Some(cfg))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("read config file {}: {e}", path.display())),
    }
}

/// Serialize `cfg` to TOML and write to `<config_dir>/config.toml` atomically with mode 0600.
pub fn save(cli_override: Option<&str>, cfg: &StoredConfig) -> Result<PathBuf> {
    let dir = config_dir(cli_override)?;
    create_dir_secure(&dir)?;
    let path = dir.join(CONFIG_FILE_NAME);
    let body = toml::to_string_pretty(cfg).context("serialize config to TOML")?;
    write_file_secure(&path, body.as_bytes())?;
    Ok(path)
}

/// Remove the config file. Returns `Ok(true)` if a file was deleted, `Ok(false)` if it
/// didn't exist.
pub fn delete(cli_override: Option<&str>) -> Result<bool> {
    let path = config_path(cli_override)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow!("remove config file {}: {e}", path.display())),
    }
}

fn create_dir_secure(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("create config dir {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dir)
            .with_context(|| format!("stat config dir {}", dir.display()))?
            .permissions();
        if perms.mode() & 0o777 != 0o700 {
            perms.set_mode(0o700);
            fs::set_permissions(dir, perms)
                .with_context(|| format!("chmod 0700 {}", dir.display()))?;
        }
    }
    Ok(())
}

fn write_file_secure(path: &Path, body: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("config path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("config path has no file name: {}", path.display()))?;
    let tmp = dir.join(format!(".{file_name}.tmp"));

    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("open temp config file {}", tmp.display()))?;
        f.write_all(body)
            .with_context(|| format!("write temp config file {}", tmp.display()))?;
        f.sync_all().ok();
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&tmp, perms)
            .with_context(|| format!("chmod 0600 {}", tmp.display()))?;
    }

    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cfg_override(t: &TempDir) -> String {
        t.path().to_string_lossy().into_owned()
    }

    #[test]
    fn load_missing_returns_none() {
        let t = TempDir::new().unwrap();
        let dir = cfg_override(&t);
        assert!(load(Some(&dir)).unwrap().is_none());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let t = TempDir::new().unwrap();
        let dir = cfg_override(&t);
        let cfg = StoredConfig {
            private_key: Some("0xabcd".into()),
            chain_id: Some(11155420),
            scope_id: Some("0x0000000000000000000000000000000000000000000000000000000000000001".into()),
            signature_type: Some("gnosis-safe".into()),
            safe_address: Some("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into()),
            network: Some("monad".into()),
            tenant: Some("hermestrade.xyz".into()),
        };
        let path = save(Some(&dir), &cfg).unwrap();
        assert!(path.exists());
        let loaded = load(Some(&dir)).unwrap().unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn delete_removes_file() {
        let t = TempDir::new().unwrap();
        let dir = cfg_override(&t);
        save(Some(&dir), &StoredConfig { private_key: Some("0x01".into()), ..Default::default() }).unwrap();
        assert!(delete(Some(&dir)).unwrap());
        assert!(!delete(Some(&dir)).unwrap());
        assert!(load(Some(&dir)).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new().unwrap();
        let dir = cfg_override(&t);
        let path = save(
            Some(&dir),
            &StoredConfig { private_key: Some("0xff".into()), ..Default::default() },
        )
        .unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file should be 0600, got {mode:o}");
        let dir_mode = fs::metadata(t.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "config dir should be 0700, got {dir_mode:o}");
    }

    #[test]
    fn env_var_fallback_used_when_no_override() {
        let t = TempDir::new().unwrap();
        // SAFETY: tests run single-threaded for this var via the explicit save/load below.
        // SAFETY: setting env vars is unsafe in Rust 2024 because other threads may be
        // reading them; this test is contained and the value is restored.
        let prev = std::env::var("PM_CONFIG_DIR").ok();
        unsafe { std::env::set_var("PM_CONFIG_DIR", t.path()) };
        let cfg = StoredConfig { chain_id: Some(42), ..Default::default() };
        save(None, &cfg).unwrap();
        let loaded = load(None).unwrap().unwrap();
        match prev {
            Some(v) => unsafe { std::env::set_var("PM_CONFIG_DIR", v) },
            None => unsafe { std::env::remove_var("PM_CONFIG_DIR") },
        }
        assert_eq!(loaded.chain_id, Some(42));
    }

    #[test]
    fn resolve_with_slug_nests_under_base() {
        let t = TempDir::new().unwrap();
        let base = cfg_override(&t);
        let dir = resolve_with_slug(Some(&base), Some("acctA")).unwrap();
        assert_eq!(dir, t.path().join("acctA"));
    }

    #[test]
    fn resolve_with_slug_none_returns_base() {
        let t = TempDir::new().unwrap();
        let base = cfg_override(&t);
        let dir = resolve_with_slug(Some(&base), None).unwrap();
        assert_eq!(dir, t.path());
    }

    #[test]
    fn resolve_with_slug_rejects_traversal_and_separators() {
        for bad in ["..", "../escape", "a/b", "/abs", "", ".", "x/../y"] {
            assert!(
                resolve_with_slug(Some("/tmp/base"), Some(bad)).is_err(),
                "slug {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn save_then_load_through_resolved_slug_dir() {
        let t = TempDir::new().unwrap();
        let base = cfg_override(&t);
        let dir = resolve_with_slug(Some(&base), Some("acctB")).unwrap();
        let dir_str = dir.to_string_lossy().into_owned();
        save(
            Some(&dir_str),
            &StoredConfig { chain_id: Some(7), ..Default::default() },
        )
        .unwrap();
        assert!(t.path().join("acctB").join(CONFIG_FILE_NAME).exists());
        assert_eq!(load(Some(&dir_str)).unwrap().unwrap().chain_id, Some(7));
    }
}
