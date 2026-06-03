//! `relayer-service` HTTP client. Submits Safe meta-transactions
//! (`Safe.execTransaction` payloads pre-signed by the EOA) and lets the relayer broadcast
//! them from its gas-key pool.
//!
//! Construct via [`crate::Client::relayer`]. Authenticated by JWT Bearer (obtained from
//! [`crate::Client::jwt_login`]) or by API-Key headers.
//!
//! Endpoints:
//! - `POST /submit` → `RelayerClient::submit` (returns `transactionID` immediately, async settlement)
//! - `GET /transaction?id=<txId>` → `RelayerClient::transaction` (poll for final state)
//!
//! Wire shape verified against the platform repo's `services/relayer-service/pkg/types/types.go`
//! and the front-end reference at `apps/user-dapp/src/hooks/useSetupSteps.ts:563`.

pub mod client;
pub mod types;

pub use client::RelayerClient;
pub use types::{
    RelayerTransaction, SafeCreateParams, SafeTxParams, SubmitRequest, SubmitResponse,
    SubmitType, TransactionState,
};
