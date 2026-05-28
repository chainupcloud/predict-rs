//! Multi-endpoint configuration. Mirrors `pm-sdk-go`'s `WithEndpoints(clob, gamma, ws)` API.
//!
//! A `pm-cup2026` tenant exposes up to four service hosts, conventionally as subdomains
//! under the tenant root:
//!
//! - **CLOB REST** (`clob-api.<tenant>`) — order book, prices, orders, auth.
//! - **Gamma REST** (`gamma-api.<tenant>`) — market metadata: events, markets, tags.
//! - **CLOB WebSocket** (`clob-ws.<tenant>`) — real-time order-book + user-channel push.
//! - **Data REST** (`data-api.<tenant>`) — portfolio, trades, activity, leaderboards.
//!   Wraps the `data-service` microservice.
//!
//! Construct an [`Endpoints`] from any of: explicit URLs, the canonical subdomain pattern
//! (`Endpoints::from_tenant("hermestrade.xyz")`), or a single CLOB URL for
//! read-only flows (`Endpoints::clob_only(...)`).

use url::Url;

use crate::error::{Error, Result};

/// Service-host configuration for a `pm-cup2026` tenant.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub clob: Url,
    pub gamma: Option<Url>,
    pub ws: Option<Url>,
    /// `data-api.<tenant>` — data-service (portfolio / trades / activity /
    /// leaderboards). Optional because read-only flows don't need it; the
    /// data sub-client will surface a validation error if it's missing.
    pub data: Option<Url>,
    /// `relayer-api.<tenant>` — relayer-service (Safe meta-tx submission).
    /// Optional; required when calling [`crate::relayer::RelayerClient`].
    pub relayer: Option<Url>,
}

impl Endpoints {
    /// All three core endpoints supplied explicitly. Use this when subdomains do not follow
    /// the canonical pattern (e.g. dev environment, custom routing). The data endpoint stays
    /// unset; use [`Self::with_data`] to attach one.
    pub fn new(
        clob: impl AsRef<str>,
        gamma: impl AsRef<str>,
        ws: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self {
            clob: parse(clob.as_ref())?,
            gamma: Some(parse(gamma.as_ref())?),
            ws: Some(parse(ws.as_ref())?),
            data: None,
            relayer: None,
        })
    }

    /// Derive all four endpoints from a tenant root host using the canonical
    /// subdomain pattern: `clob-api.<host>` / `gamma-api.<host>` / `clob-ws.<host>` (ws over
    /// TLS) / `data-api.<host>`. For example: `Endpoints::from_tenant("hermestrade.xyz")`
    /// resolves to `https://clob-api.hermestrade.xyz`, `https://gamma-api.hermestrade.xyz`,
    /// `wss://clob-ws.hermestrade.xyz`, `https://data-api.hermestrade.xyz`.
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
        let mut ep = Self::new(
            format!("https://clob-api.{bare}"),
            format!("https://gamma-api.{bare}"),
            format!("wss://clob-ws.{bare}"),
        )?;
        ep.data = Some(parse(&format!("https://data-api.{bare}"))?);
        ep.relayer = Some(parse(&format!("https://relayer-api.{bare}"))?);
        Ok(ep)
    }

    /// CLOB-only — useful for read-only smoke tests where Gamma / WS / Data are not exercised.
    pub fn clob_only(clob: impl AsRef<str>) -> Result<Self> {
        Ok(Self {
            clob: parse(clob.as_ref())?,
            gamma: None,
            ws: None,
            data: None,
            relayer: None,
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

    /// Attach a data-service URL (`data-api.<tenant>`). Required to use [`crate::data::DataClient`].
    #[must_use]
    pub fn with_data(mut self, data: Url) -> Self {
        self.data = Some(data);
        self
    }

    /// Attach a relayer-service URL (`relayer-api.<tenant>`). Required to use
    /// [`crate::relayer::RelayerClient`].
    #[must_use]
    pub fn with_relayer(mut self, relayer: Url) -> Self {
        self.relayer = Some(relayer);
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
        assert_eq!(ep.data.as_ref().unwrap().as_str(), "https://data-api.hermestrade.xyz/");
        assert_eq!(ep.relayer.as_ref().unwrap().as_str(), "https://relayer-api.hermestrade.xyz/");
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
