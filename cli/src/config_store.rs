//! Persistent CLI configuration on disk.
//!
//! The store backs `pm wallet` and is read as a fallback by every command that needs a
//! private key / chain id / scope id when those values are not supplied via flag or env.
//!
//! Resolution for the config directory:
//!   1. `--config-dir <path>` flag
//!   2. `PM_CONFIG_DIR` env var
//!   3. `dirs::config_dir()/pm` (Linux: `~/.config/pm`, macOS: `~/Library/Application Support/pm`)
//!
//! The store is a single TOML file `config.toml` inside that directory. Writes are atomic
//! (write to a sibling temp file then rename) and the file is created with mode 0600 on
//! Unix; the parent directory is created with mode 0700.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

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
    Ok(base.join("pm"))
}

pub fn config_path(cli_override: Option<&str>) -> Result<PathBuf> {
    Ok(config_dir(cli_override)?.join(CONFIG_FILE_NAME))
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
}
