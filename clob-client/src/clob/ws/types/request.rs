//! Outbound subscription / control envelopes.
//!
//! Field names and constants match `asyncapi-{market,user}.json` byte-for-byte;
//! the `pm-sdk-go` reference (`pkg/ws/types.go`) is the canonical Go-side
//! counterpart.

use serde::{Deserialize, Serialize};

/// Order-book depth level a `/ws/market` subscriber may request.
///
/// The chainup server treats unknown / `0` as the default (`Two`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum MarketLevel {
    One = 1,
    Two = 2,
    Three = 3,
}

impl From<MarketLevel> for u8 {
    fn from(l: MarketLevel) -> Self {
        l as u8
    }
}

impl TryFrom<u8> for MarketLevel {
    type Error = String;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            other => Err(format!("invalid market level {other}: expected 1, 2, or 3")),
        }
    }
}

/// Initial subscription envelope for `/ws/market`. Matches the
/// `subscriptionRequest` schema in `asyncapi-market.json`.
///
/// The server interprets a missing `initial_dump` as `true` and a missing
/// `level` as `2`; we skip both when `None` so the wire output stays minimal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSubscribeRequest {
    #[serde(rename = "assets_ids")]
    pub assets_ids: Vec<String>,
    #[serde(rename = "type")]
    pub r#type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_dump: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<MarketLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_feature_enabled: Option<bool>,
}

impl MarketSubscribeRequest {
    /// Build a fresh subscription envelope with `type = "market"`.
    #[must_use]
    pub fn new(asset_ids: Vec<String>) -> Self {
        Self {
            assets_ids: asset_ids,
            r#type: "market",
            initial_dump: None,
            level: None,
            custom_feature_enabled: None,
        }
    }
}

/// Runtime subscribe / unsubscribe envelope for `/ws/market`. Matches the
/// `subscriptionRequestUpdate` schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketUpdateRequest {
    pub operation: SubscriptionOperation,
    #[serde(rename = "assets_ids")]
    pub assets_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<MarketLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_feature_enabled: Option<bool>,
}

/// Authenticated subscribe envelope for `/ws/user`. The server reads the
/// `auth.apiKey` + `auth.passphrase` fields from the *first* WS frame; HTTP
/// headers are ignored (see `wsservice/user_channel.go`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSubscribeRequest {
    pub auth: UserAuth,
    #[serde(rename = "type")]
    pub r#type: &'static str,
    /// Condition IDs to filter by; the server treats an empty array as
    /// "all markets".
    pub markets: Vec<String>,
}

impl UserSubscribeRequest {
    #[must_use]
    pub fn new(api_key: String, passphrase: String, markets: Vec<String>) -> Self {
        Self {
            auth: UserAuth { api_key, passphrase, secret: None },
            r#type: "user",
            markets,
        }
    }
}

/// Runtime subscribe / unsubscribe envelope for `/ws/user`. Matches the
/// `userSubscriptionRequestUpdate` schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdateRequest {
    pub operation: SubscriptionOperation,
    pub markets: Vec<String>,
}

/// Auth credentials shipped in the user-channel subscribe envelope.
///
/// `secret` is currently unused by the server (it validates passphrase against
/// the stored API key only) but is accepted; we omit it to keep the wire
/// frame compact unless a caller explicitly fills it in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAuth {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub passphrase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionOperation {
    Subscribe,
    Unsubscribe,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_subscribe_minimal() {
        let req = MarketSubscribeRequest::new(vec!["abc".into(), "def".into()]);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "assets_ids": ["abc", "def"],
                "type": "market"
            })
        );
    }

    #[test]
    fn market_subscribe_full() {
        let req = MarketSubscribeRequest {
            assets_ids: vec!["x".into()],
            r#type: "market",
            initial_dump: Some(true),
            level: Some(MarketLevel::One),
            custom_feature_enabled: Some(true),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "assets_ids": ["x"],
                "type": "market",
                "initial_dump": true,
                "level": 1,
                "custom_feature_enabled": true,
            })
        );
    }

    #[test]
    fn market_update_round_trip() {
        let req = MarketUpdateRequest {
            operation: SubscriptionOperation::Unsubscribe,
            assets_ids: vec!["1".into()],
            level: None,
            custom_feature_enabled: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"operation\":\"unsubscribe\""));
        assert!(json.contains("\"assets_ids\":[\"1\"]"));
        assert!(!json.contains("level"));
    }

    #[test]
    fn user_subscribe_carries_auth_in_body() {
        let req = UserSubscribeRequest::new(
            "key".into(),
            "pass".into(),
            vec!["0xcid".into()],
        );
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["auth"]["apiKey"], "key");
        assert_eq!(json["auth"]["passphrase"], "pass");
        assert_eq!(json["type"], "user");
        assert_eq!(json["markets"][0], "0xcid");
        // The optional secret must not appear when unset.
        assert!(json["auth"].get("secret").is_none());
    }
}
