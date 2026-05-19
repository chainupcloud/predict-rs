//! Multi-endpoint configuration. Mirrors `pm-sdk-go`'s `WithEndpoints(clob, gamma, ws)` API.
//!
//! A `pm-cup2026` tenant exposes three service hosts, conventionally as subdomains under the
//! tenant root (e.g. `clob-api.<tenant>`, `gamma-api.<tenant>`, `clob-ws.<tenant>`):
//!
//! - **CLOB REST** (`clob-api.<tenant>`) — order book, prices, orders, auth.
//! - **Gamma REST** (`gamma-api.<tenant>`) — market metadata: events, markets, tags.
//! - **CLOB WebSocket** (`clob-ws.<tenant>`) — real-time order-book + user-channel push.
//!
//! Construct an [`Endpoints`] from any of: three explicit URLs, the canonical subdomain
//! pattern (`Endpoints::from_tenant("hermestrade.xyz")`), or a single CLOB URL for Phase 1
//! read-only flows (`Endpoints::clob_only(...)`).

use url::Url;

use crate::error::{Error, Result};

/// Service-host triple for a `pm-cup2026` tenant.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub clob: Url,
    pub gamma: Option<Url>,
    pub ws: Option<Url>,
}

impl Endpoints {
    /// All three endpoints supplied explicitly. Use this when subdomains do not follow the
    /// canonical pattern (e.g. dev environment, custom routing).
    pub fn new(
        clob: impl AsRef<str>,
        gamma: impl AsRef<str>,
        ws: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self {
            clob: parse(clob.as_ref())?,
            gamma: Some(parse(gamma.as_ref())?),
            ws: Some(parse(ws.as_ref())?),
        })
    }

    /// Derive all three endpoints from a tenant root host using the chainup canonical
    /// subdomain pattern: `clob-api.<host>` / `gamma-api.<host>` / `clob-ws.<host>` (ws over
    /// TLS). For example: `Endpoints::from_tenant("hermestrade.xyz")` resolves to
    /// `https://clob-api.hermestrade.xyz`, `https://gamma-api.hermestrade.xyz`,
    /// `wss://clob-ws.hermestrade.xyz`.
    pub fn from_tenant(host: impl AsRef<str>) -> Result<Self> {
        let raw = host.as_ref();
        // Strip protocol prefix first so the trailing-slash trim below sees only the host.
        let stripped = raw
            .strip_prefix("https://")
            .or_else(|| raw.strip_prefix("http://"))
            .or_else(|| raw.strip_prefix("wss://"))
            .or_else(|| raw.strip_prefix("ws://"))
            .unwrap_or(raw);
        let bare = stripped.trim_end_matches('/');
        if bare.is_empty() {
            return Err(Error::validation("tenant host is empty"));
        }
        Self::new(
            format!("https://clob-api.{bare}"),
            format!("https://gamma-api.{bare}"),
            format!("wss://clob-ws.{bare}"),
        )
    }

    /// CLOB-only — useful for Phase 1 read-only smoke tests where Gamma / WS are not exercised.
    pub fn clob_only(clob: impl AsRef<str>) -> Result<Self> {
        Ok(Self {
            clob: parse(clob.as_ref())?,
            gamma: None,
            ws: None,
        })
    }

    /// Replace the CLOB URL (chaining helper).
    #[must_use]
    pub fn with_clob(mut self, clob: Url) -> Self {
        self.clob = clob;
        self
    }

    /// Replace the Gamma URL.
    #[must_use]
    pub fn with_gamma(mut self, gamma: Url) -> Self {
        self.gamma = Some(gamma);
        self
    }

    /// Replace the WebSocket URL.
    #[must_use]
    pub fn with_ws(mut self, ws: Url) -> Self {
        self.ws = Some(ws);
        self
    }
}

fn parse(s: &str) -> Result<Url> {
    let mut s = s.to_owned();
    // Ensure trailing slash so `base.join(path)` behaves correctly for relative paths.
    if !s.ends_with('/') {
        s.push('/');
    }
    Ok(Url::parse(&s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_tenant_canonical_subdomains() {
        let ep = Endpoints::from_tenant("hermestrade.xyz").unwrap();
        assert_eq!(ep.clob.as_str(), "https://clob-api.hermestrade.xyz/");
        assert_eq!(ep.gamma.as_ref().unwrap().as_str(), "https://gamma-api.hermestrade.xyz/");
        assert_eq!(ep.ws.as_ref().unwrap().as_str(), "wss://clob-ws.hermestrade.xyz/");
    }

    #[test]
    fn from_tenant_strips_protocol_prefix() {
        let ep = Endpoints::from_tenant("https://hermestrade.xyz/").unwrap();
        assert_eq!(ep.clob.as_str(), "https://clob-api.hermestrade.xyz/");
    }

    #[test]
    fn from_tenant_rejects_empty() {
        assert!(Endpoints::from_tenant("").is_err());
        assert!(Endpoints::from_tenant("https://").is_err());
    }

    #[test]
    fn explicit_three_url_form() {
        let ep = Endpoints::new(
            "https://clob.example.com",
            "https://gamma.example.com",
            "wss://ws.example.com",
        )
        .unwrap();
        assert_eq!(ep.clob.as_str(), "https://clob.example.com/");
    }
}
