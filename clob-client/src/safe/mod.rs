//! Gnosis Safe transaction primitives — the EIP-712 `SafeTx` struct, the MultiSend
//! packed encoder, and the digest computation needed for [`crate::PMCup26Signer::sign_safe_tx`].
//!
//! Two scenarios that consume this module:
//!
//! 1. **Single-op Safe meta-tx** — e.g. `USDW.approve(spender, MAX)`. Build a [`SafeTransaction`]
//!    with `to = USDW`, `data = approve(...)`, `operation = SafeOperation::Call`, sign it,
//!    and submit via [`crate::relayer::RelayerClient::submit`].
//!
//! 2. **Batched MultiSend** — N operations executed atomically as one Safe.execTransaction.
//!    Build a `Vec<SafeSubOp>`, encode via [`multisend::encode`], then put the resulting
//!    calldata in a [`SafeTransaction`] with `to = multiSendAddress`,
//!    `operation = SafeOperation::DelegateCall`.
//!
//! All wire formats match Gnosis Safe v1.3 — the version chainup deploys.

pub mod multisend;

use alloy::primitives::{Address, U256};

/// Operation flavour for a Safe transaction. Maps to the `operation` field on
/// `Safe.execTransaction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeOperation {
    /// `CALL` — standard contract call. Used for direct ops like `usdc.approve`.
    Call = 0,
    /// `DELEGATECALL` — executes the target's code in the Safe's context. Required when
    /// `to == multiSendAddress` so the batched ops appear as the Safe itself.
    DelegateCall = 1,
}

/// One Safe `SafeTx` payload — the fields signed under the `SafeTx` EIP-712 type and
/// submitted to `Safe.execTransaction`.
///
/// Field order matches Gnosis Safe v1.3 exactly. Missing `signatures` field — that's
/// supplied separately by the signer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeTransaction {
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub operation: SafeOperation,
    pub safe_tx_gas: U256,
    pub base_gas: U256,
    pub gas_price: U256,
    pub gas_token: Address,
    pub refund_receiver: Address,
    pub nonce: U256,
}

impl SafeTransaction {
    /// Construct a single-call SafeTx with zero gas params (relayer-pays mode — the
    /// chainup `relayer-service` ignores the gas fields and uses its own gas-key pool).
    #[must_use]
    pub fn call(to: Address, data: Vec<u8>, nonce: U256) -> Self {
        Self {
            to,
            value: U256::ZERO,
            data,
            operation: SafeOperation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce,
        }
    }

    /// Construct a DelegateCall SafeTx — needed for MultiSend-batched ops.
    #[must_use]
    pub fn delegate_call(multisend_address: Address, packed_data: Vec<u8>, nonce: U256) -> Self {
        Self {
            to: multisend_address,
            value: U256::ZERO,
            data: packed_data,
            operation: SafeOperation::DelegateCall,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce,
        }
    }
}
