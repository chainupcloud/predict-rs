//! Request / response types for the `relayer-service` REST API.
//!
//! Field names and JSON tags mirror the platform repo's `services/relayer-service/pkg/types/types.go`
//! 1:1 (camelCase wire format).

use serde::{Deserialize, Serialize};

/// `SubmitRequest.type` values supported by the relayer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubmitType {
    /// Execute arbitrary calls through an existing Safe via `Safe.execTransaction`.
    #[serde(rename = "SAFE")]
    Safe,
    /// Create a fresh user Safe via `SafeProxyFactory.createProxy`.
    #[serde(rename = "SAFE-CREATE")]
    SafeCreate,
}

/// `signatureParams` for a `SUBMIT type = SAFE` request — the leftover SafeTx fields the
/// relayer needs to reconstruct the on-chain payload (everything other than
/// `to / value / data / operation / nonce` which are top-level on the request).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafeTxParams {
    pub gas_price: String,
    /// "0" = CALL, "1" = DELEGATECALL.
    pub operation: String,
    #[serde(rename = "safeTxnGas")]
    pub safe_txn_gas: String,
    pub base_gas: String,
    pub gas_token: String,
    pub refund_receiver: String,
}

impl SafeTxParams {
    /// Zero-gas defaults (relayer-pays mode). The relayer ignores these fields in practice
    /// — they exist to match Safe.execTransaction's signature, and the relayer's gas key
    /// pool pays the actual on-chain gas.
    #[must_use]
    pub fn relayer_pays(operation_delegatecall: bool) -> Self {
        Self {
            gas_price: "0".into(),
            operation: if operation_delegatecall { "1".into() } else { "0".into() },
            safe_txn_gas: "0".into(),
            base_gas: "0".into(),
            gas_token: "0x0000000000000000000000000000000000000000".into(),
            refund_receiver: "0x0000000000000000000000000000000000000000".into(),
        }
    }
}

/// `signatureParams` for a `SUBMIT type = SAFE-CREATE` request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafeCreateParams {
    pub payment_token: String,
    pub payment: String,
    pub payment_receiver: String,
    pub scope_id: String,
}

/// `POST /submit` request envelope. Field shapes match the Go struct at
/// `services/relayer-service/pkg/types/types.go:50` exactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitRequest {
    /// EOA address — must match the authenticated identity. Checksummed hex with `0x` prefix.
    pub from: String,
    /// Target contract. For `SAFE` type, must be in the relayer's whitelist; for MultiSend
    /// batches, set to the configured MultiSend contract address (and each inner op's `to`
    /// will be validated independently).
    pub to: String,
    /// Safe address (the proxy wallet through which the call executes).
    pub proxy_wallet: String,
    /// Inner calldata — for a single op, the encoded function call; for MultiSend, the
    /// output of [`crate::safe::multisend::encode`].
    pub data: String,
    /// Safe.nonce() at sign time (decimal string). Optional for `SAFE-CREATE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// 65-byte EIP-712 SafeTx signature, `0x`-prefixed hex.
    pub signature: String,
    /// Type-specific params. Use [`SafeTxParams`] for `SAFE`, [`SafeCreateParams`] for
    /// `SAFE-CREATE` — serialised as raw JSON so the same request type covers both flows.
    pub signature_params: serde_json::Value,
    #[serde(rename = "type")]
    pub r#type: SubmitType,
    /// Tenant scope id — `0x`-prefixed bytes32 hex. Required for JWT-auth requests
    /// (must match the JWT's `scope_id` claim).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    /// Free-form tag carried into audit logs (`approval` / `redeem` / etc.). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

/// `POST /submit` immediate response. The `transaction_hash` is empty until the relayer
/// actually broadcasts; poll `GET /transaction?id=<transaction_id>` until [`TransactionState`]
/// reaches a terminal value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResponse {
    #[serde(rename = "transactionID")]
    pub transaction_id: String,
    #[serde(default)]
    pub transaction_hash: String,
    pub state: TransactionState,
}

/// Lifecycle state for a relayer-tracked transaction. Values mirror `relayer-service`'s
/// internal `tx.State` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionState {
    #[serde(rename = "STATE_NEW")]
    New,
    #[serde(rename = "STATE_QUEUED")]
    Queued,
    #[serde(rename = "STATE_SENT")]
    Sent,
    #[serde(rename = "STATE_MINED")]
    Mined,
    #[serde(rename = "STATE_CONFIRMED")]
    Confirmed,
    #[serde(rename = "STATE_FAILED")]
    Failed,
    #[serde(rename = "STATE_EXECUTED")]
    Executed,
    #[serde(rename = "STATE_DROPPED")]
    Dropped,
}

impl TransactionState {
    /// Returns true for the two terminal states a caller should stop polling on.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Confirmed | Self::Failed | Self::Dropped)
    }
}

/// `GET /transaction?id=<txId>` response. Field names match the Go relayer's full
/// `RelayerTransaction` row; lean fields default-empty for forward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayerTransaction {
    #[serde(rename = "transactionID")]
    pub transaction_id: String,
    #[serde(default)]
    pub transaction_hash: String,
    pub state: TransactionState,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub proxy_wallet: String,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub block_number: Option<u64>,
    #[serde(default)]
    pub gas_used: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_type_serializes_with_dash() {
        assert_eq!(serde_json::to_string(&SubmitType::Safe).unwrap(), r#""SAFE""#);
        assert_eq!(
            serde_json::to_string(&SubmitType::SafeCreate).unwrap(),
            r#""SAFE-CREATE""#
        );
    }

    #[test]
    fn safe_tx_params_camelcase_round_trip() {
        let p = SafeTxParams::relayer_pays(true);
        let json = serde_json::to_string(&p).unwrap();
        // Note `safeTxnGas` (typo present on the server side — the Go struct spelling).
        assert!(json.contains("\"safeTxnGas\":\"0\""));
        assert!(json.contains("\"operation\":\"1\""));
        let back: SafeTxParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn submit_request_serializes_with_camelcase() {
        let req = SubmitRequest {
            from: "0xeoa".into(),
            to: "0xmultisend".into(),
            proxy_wallet: "0xsafe".into(),
            data: "0xdeadbeef".into(),
            nonce: Some("3".into()),
            signature: "0xsig".into(),
            signature_params: serde_json::to_value(SafeTxParams::relayer_pays(true)).unwrap(),
            r#type: SubmitType::Safe,
            scope_id: Some("0xscope".into()),
            metadata: Some("approval".into()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["proxyWallet"], "0xsafe");
        assert_eq!(json["signatureParams"]["operation"], "1");
        assert_eq!(json["type"], "SAFE");
        assert_eq!(json["scopeId"], "0xscope");
        assert_eq!(json["metadata"], "approval");
    }

    #[test]
    fn transaction_state_is_terminal_classifies_correctly() {
        assert!(TransactionState::Confirmed.is_terminal());
        assert!(TransactionState::Failed.is_terminal());
        assert!(TransactionState::Dropped.is_terminal());
        assert!(!TransactionState::New.is_terminal());
        assert!(!TransactionState::Sent.is_terminal());
        assert!(!TransactionState::Mined.is_terminal());
    }

    #[test]
    fn submit_response_decodes_initial_state() {
        let raw = r#"{"transactionID":"abc-123","transactionHash":"","state":"STATE_NEW"}"#;
        let r: SubmitResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(r.transaction_id, "abc-123");
        assert!(r.transaction_hash.is_empty());
        assert_eq!(r.state, TransactionState::New);
    }
}
