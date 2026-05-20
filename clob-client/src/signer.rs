//! ChainUp pm-cup2026 EIP-712 signer.
//!
//! Produces byte-identical signatures with `pm-sdk-go/pkg/signer` for:
//!
//! - **ClobAuth** (L1 auth): 5-field struct with `bytes32 scopeId` and short-form
//!   `EIP712Domain(string name,string version,uint256 chainId)`.
//! - **Order**: 13-field struct with `bytes32 scopeId` at the end and full-form
//!   `EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)`.
//!
//! Verified against `pm-sdk-go/pkg/signer/testdata/golden.json` in
//! `tests/golden_signer.rs`.

use std::borrow::Cow;

use alloy::dyn_abi::Eip712Domain;
use alloy::primitives::{Address, B256, U256, keccak256};
use alloy::signers::{Signature, SignerSync};
use alloy::signers::local::LocalSigner;
use alloy_sol_types::{SolStruct, sol};

use crate::error::{Error, Result};
use crate::types::ScopeId;

/// EIP-712 domain name for the ClobAuth L1 challenge.
pub const CLOB_AUTH_DOMAIN_NAME: &str = "ClobAuthDomain";

/// EIP-712 domain name for the CTFExchange order. Note: domain name is
/// `"Prediction Market Protocol"` (NOT `"CTFExchange"` or `"Polymarket CTF Exchange"` — chainup
/// chose this name on its on-chain contract and the SDK must match exactly or signature
/// verification fails on-chain).
pub const ORDER_DOMAIN_NAME: &str = "Prediction Market Protocol";

/// EIP-712 domain version. Both ClobAuth and Order share `"1"`.
pub const DOMAIN_VERSION: &str = "1";

/// L1 ClobAuth challenge message.
pub const CLOB_AUTH_MESSAGE: &str = "This message attests that I control the given wallet";

sol! {
    /// EIP-712 struct for the L1 ClobAuth challenge.
    /// **Field order is part of the protocol** — must match
    /// `services/clob-service/internal/shared/crypto/eip712.go::clobAuthTypeHash`.
    #[derive(Debug)]
    struct ClobAuth {
        address address;
        string  timestamp;
        uint256 nonce;
        bytes32 scopeId;
        string  message;
    }
}

sol! {
    /// EIP-712 struct for an exchange order (CTFExchange).
    /// **Field order is part of the protocol** — must match
    /// `services/clob-service/internal/shared/crypto/order_eip712.go::orderTypeHash`.
    #[derive(Debug)]
    struct Order {
        uint256 salt;
        address maker;
        address signer;
        address taker;
        uint256 tokenId;
        uint256 makerAmount;
        uint256 takerAmount;
        uint256 expiration;
        uint256 nonce;
        uint256 feeRateBps;
        uint8   side;
        uint8   signatureType;
        bytes32 scopeId;
    }
}

sol! {
    /// EIP-712 struct for a Gnosis Safe transaction. Field order matches Safe v1.3 exactly
    /// — `services/relayer-service` and the front-end signing code at
    /// `apps/user-dapp/src/hooks/useSetupSteps.ts:543` both depend on this layout.
    #[derive(Debug)]
    struct SafeTx {
        address to;
        uint256 value;
        bytes   data;
        uint8   operation;
        uint256 safeTxGas;
        uint256 baseGas;
        uint256 gasPrice;
        address gasToken;
        address refundReceiver;
        uint256 nonce;
    }
}

/// EIP-712 domain for a Safe meta-tx — no `name`, no `version`, just `chainId` +
/// `verifyingContract` (the Safe address). This is the Safe v1.3 convention.
fn safe_tx_domain(chain_id: u64, safe: Address) -> Eip712Domain {
    Eip712Domain {
        name: None,
        version: None,
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: Some(safe),
        salt: None,
    }
}

/// Compute the 32-byte SafeTx EIP-712 digest for the given Safe address.
#[must_use]
pub fn safe_tx_digest(tx: &crate::safe::SafeTransaction, safe: Address, chain_id: u64) -> B256 {
    let domain = safe_tx_domain(chain_id, safe);
    let sol = SafeTx {
        to: tx.to,
        value: tx.value,
        data: tx.data.clone().into(),
        operation: tx.operation as u8,
        safeTxGas: tx.safe_tx_gas,
        baseGas: tx.base_gas,
        gasPrice: tx.gas_price,
        gasToken: tx.gas_token,
        refundReceiver: tx.refund_receiver,
        nonce: tx.nonce,
    };
    sol.eip712_signing_hash(&domain)
}

/// Plain-data Order used by callers (free of alloy's sol! generated type, easier for FFI / serde).
#[derive(Debug, Clone)]
pub struct OrderForSigning {
    pub salt: U256,
    pub maker: Address,
    pub signer: Address,
    pub taker: Address,
    pub token_id: U256,
    pub maker_amount: U256,
    pub taker_amount: U256,
    pub expiration: u64,
    pub nonce: u64,
    pub fee_rate_bps: u64,
    pub side: u8,
    pub signature_type: u8,
    pub scope_id: ScopeId,
}

impl OrderForSigning {
    fn to_sol(&self) -> Order {
        Order {
            salt: self.salt,
            maker: self.maker,
            signer: self.signer,
            taker: self.taker,
            tokenId: self.token_id,
            makerAmount: self.maker_amount,
            takerAmount: self.taker_amount,
            expiration: U256::from(self.expiration),
            nonce: U256::from(self.nonce),
            feeRateBps: U256::from(self.fee_rate_bps),
            side: self.side,
            signatureType: self.signature_type,
            scopeId: self.scope_id.as_b256(),
        }
    }
}

/// Build the short-form EIP-712 domain used by ClobAuth (no `verifyingContract`).
#[must_use]
pub fn clob_auth_domain(chain_id: u64) -> Eip712Domain {
    Eip712Domain {
        name: Some(Cow::Borrowed(CLOB_AUTH_DOMAIN_NAME)),
        version: Some(Cow::Borrowed(DOMAIN_VERSION)),
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: None,
        salt: None,
    }
}

/// Build the full EIP-712 domain used by Order (with `verifyingContract`).
#[must_use]
pub fn order_domain(chain_id: u64, exchange: Address) -> Eip712Domain {
    Eip712Domain {
        name: Some(Cow::Borrowed(ORDER_DOMAIN_NAME)),
        version: Some(Cow::Borrowed(DOMAIN_VERSION)),
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: Some(exchange),
        salt: None,
    }
}

/// Compute the 32-byte ClobAuth EIP-712 digest (sign-ready).
#[must_use]
pub fn clob_auth_digest(address: Address, timestamp: &str, nonce: u64, scope_id: ScopeId, chain_id: u64) -> B256 {
    let domain = clob_auth_domain(chain_id);
    let auth = ClobAuth {
        address,
        timestamp: timestamp.to_owned(),
        nonce: U256::from(nonce),
        scopeId: scope_id.as_b256(),
        message: CLOB_AUTH_MESSAGE.to_owned(),
    };
    auth.eip712_signing_hash(&domain)
}

/// Compute the 32-byte ClobAuth domain separator. Useful for golden tests.
#[must_use]
pub fn clob_auth_domain_separator(chain_id: u64) -> B256 {
    clob_auth_domain(chain_id).separator()
}

/// Compute the 32-byte ClobAuth struct hash. Useful for golden tests.
#[must_use]
pub fn clob_auth_struct_hash(address: Address, timestamp: &str, nonce: u64, scope_id: ScopeId) -> B256 {
    let auth = ClobAuth {
        address,
        timestamp: timestamp.to_owned(),
        nonce: U256::from(nonce),
        scopeId: scope_id.as_b256(),
        message: CLOB_AUTH_MESSAGE.to_owned(),
    };
    auth.eip712_hash_struct()
}

/// Compute the 32-byte Order EIP-712 digest.
#[must_use]
pub fn order_digest(order: &OrderForSigning, exchange: Address, chain_id: u64) -> B256 {
    let domain = order_domain(chain_id, exchange);
    order.to_sol().eip712_signing_hash(&domain)
}

/// Compute the 32-byte Order domain separator.
#[must_use]
pub fn order_domain_separator(chain_id: u64, exchange: Address) -> B256 {
    order_domain(chain_id, exchange).separator()
}

/// Compute the 32-byte Order struct hash.
#[must_use]
pub fn order_struct_hash(order: &OrderForSigning) -> B256 {
    order.to_sol().eip712_hash_struct()
}

/// High-level signer that owns a private key plus the chain-level context
/// (`chain_id`, `scope_id`, and optional `exchange_address`).
///
/// Built on top of [`alloy::signers::local::LocalSigner`] (RFC 6979 deterministic ECDSA over
/// secp256k1) — produces byte-identical signatures with `go-ethereum/crypto` and `pm-sdk-go`.
#[derive(Debug, Clone)]
pub struct PMCup26Signer {
    inner: LocalSigner<alloy::signers::k256::ecdsa::SigningKey>,
    address: Address,
    chain_id: u64,
    scope_id: ScopeId,
    exchange: Option<Address>,
}

impl PMCup26Signer {
    /// Construct from a 32-byte secp256k1 private key.
    ///
    /// `chain_id` is required (used for both ClobAuth and Order domain). `scope_id` defaults to
    /// [`ScopeId::ZERO`]; use [`Self::with_scope_id`] to bind to a tenant scope. The exchange
    /// address is only needed for [`Self::sign_order`] — use [`Self::with_exchange`] to set it.
    pub fn from_private_key(privkey: &[u8; 32], chain_id: u64) -> Result<Self> {
        let inner = LocalSigner::from_bytes(&B256::from(*privkey))
            .map_err(|e| Error::signer(format!("invalid private key: {e}")))?;
        let address = inner.address();
        Ok(Self {
            inner,
            address,
            chain_id,
            scope_id: ScopeId::ZERO,
            exchange: None,
        })
    }

    /// Parse from a hex-encoded private key ("0x..." or bare hex).
    pub fn from_hex(hex_str: &str, chain_id: u64) -> Result<Self> {
        let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let bytes = hex::decode(stripped)?;
        if bytes.len() != 32 {
            return Err(Error::signer(format!(
                "private key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&bytes);
        Self::from_private_key(&buf, chain_id)
    }

    #[must_use]
    pub fn with_scope_id(mut self, scope_id: ScopeId) -> Self {
        self.scope_id = scope_id;
        self
    }

    #[must_use]
    pub fn with_exchange(mut self, exchange: Address) -> Self {
        self.exchange = Some(exchange);
        self
    }

    #[must_use]
    pub fn address(&self) -> Address {
        self.address
    }

    #[must_use]
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    #[must_use]
    pub fn scope_id(&self) -> ScopeId {
        self.scope_id
    }

    /// Sign a pre-computed 32-byte digest (low-level escape hatch).
    pub fn sign_digest(&self, digest: B256) -> Result<Signature> {
        self.inner
            .sign_hash_sync(&digest)
            .map_err(|e| Error::signer(format!("sign_hash: {e}")))
    }

    /// Sign the ClobAuth L1 challenge and return a 65-byte `r||s||v` signature.
    ///
    /// `v` is normalized to `0x00 / 0x01` (chainup / pm-sdk-go convention — matches
    /// `go-ethereum/crypto.Sign` which returns recovery_id in {0,1}).
    pub fn sign_clob_auth(&self, timestamp: &str, nonce: u64) -> Result<[u8; 65]> {
        let digest = clob_auth_digest(self.address, timestamp, nonce, self.scope_id, self.chain_id);
        let sig = self.sign_digest(digest)?;
        Ok(signature_to_bytes(sig))
    }

    /// Sign an order and return a 65-byte `r||s||v` signature.
    ///
    /// Requires `exchange_address` to be set via [`Self::with_exchange`].
    pub fn sign_order(&self, order: &OrderForSigning) -> Result<[u8; 65]> {
        let exchange = self
            .exchange
            .ok_or_else(|| Error::signer("exchange address required for sign_order"))?;
        let digest = order_digest(order, exchange, self.chain_id);
        let sig = self.sign_digest(digest)?;
        Ok(signature_to_bytes(sig))
    }

    /// Sign a Gnosis Safe transaction and return a 65-byte `r||s||v` signature with `v`
    /// in `{0x1b, 0x1c}` (Safe's on-chain verifier requires Ethereum-style v, not the
    /// `{0, 1}` recovery-id convention `pm-sdk-go` uses for ClobAuth / Order).
    ///
    /// The `safe` argument is the Safe address — used as the `verifyingContract` of the
    /// SafeTx EIP-712 domain, NOT the signer's own address. The signer (this struct's
    /// underlying EOA) must be a recognised owner of that Safe for the signature to be
    /// accepted by `Safe.execTransaction`.
    pub fn sign_safe_tx(
        &self,
        safe: Address,
        tx: &crate::safe::SafeTransaction,
    ) -> Result<[u8; 65]> {
        let digest = safe_tx_digest(tx, safe, self.chain_id);
        let sig = self.sign_digest(digest)?;
        Ok(signature_to_bytes_ethereum_v(sig))
    }
}

/// Serialize a 65-byte signature in `r || s || v` order, where `v` is the recovery id
/// normalized to `{0x00, 0x01}` (NOT EIP-155 / EIP-1559's `{0x1b, 0x1c}`).
///
/// Matches go-ethereum's `crypto.Sign` output, which is what pm-sdk-go produces.
fn signature_to_bytes(sig: Signature) -> [u8; 65] {
    let mut out = [0u8; 65];
    let r = sig.r();
    let s = sig.s();
    // r is U256 (big-endian)
    out[..32].copy_from_slice(&r.to_be_bytes::<32>());
    out[32..64].copy_from_slice(&s.to_be_bytes::<32>());
    out[64] = u8::from(sig.v());
    out
}

/// Like [`signature_to_bytes`] but with `v` shifted into the Ethereum convention
/// (`{0x1b, 0x1c}` = 27/28). Required by Safe.execTransaction's on-chain verifier
/// (`Safe.checkSignatures` calls `ecrecover` which expects 27/28).
fn signature_to_bytes_ethereum_v(sig: Signature) -> [u8; 65] {
    let mut out = signature_to_bytes(sig);
    out[64] += 27;
    out
}

/// keccak256 helper re-export for tests.
#[must_use]
pub fn keccak(data: &[u8]) -> B256 {
    keccak256(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: verify that the ClobAuth type hash matches the literal string we expect
    /// (i.e. alloy's sol! macro is computing the same thing).
    #[test]
    fn clob_auth_type_hash_matches_literal() {
        let want = keccak256(
            "ClobAuth(address address,string timestamp,uint256 nonce,bytes32 scopeId,string message)",
        );
        assert_eq!(ClobAuth::eip712_type_hash(&ClobAuth {
            address: Address::ZERO,
            timestamp: String::new(),
            nonce: U256::ZERO,
            scopeId: B256::ZERO,
            message: String::new(),
        }), want);
    }

    #[test]
    fn order_type_hash_matches_literal() {
        let want = keccak256(
            "Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType,bytes32 scopeId)",
        );
        let zero_order = Order {
            salt: U256::ZERO,
            maker: Address::ZERO,
            signer: Address::ZERO,
            taker: Address::ZERO,
            tokenId: U256::ZERO,
            makerAmount: U256::ZERO,
            takerAmount: U256::ZERO,
            expiration: U256::ZERO,
            nonce: U256::ZERO,
            feeRateBps: U256::ZERO,
            side: 0,
            signatureType: 0,
            scopeId: B256::ZERO,
        };
        assert_eq!(zero_order.eip712_type_hash(), want);
    }

    #[test]
    fn safe_tx_type_hash_matches_literal() {
        // The on-chain Safe v1.3 SafeTx type-hash is:
        //   keccak256("SafeTx(address to,uint256 value,bytes data,uint8 operation,uint256 safeTxGas,uint256 baseGas,uint256 gasPrice,address gasToken,address refundReceiver,uint256 nonce)")
        let want = keccak256(
            "SafeTx(address to,uint256 value,bytes data,uint8 operation,uint256 safeTxGas,uint256 baseGas,uint256 gasPrice,address gasToken,address refundReceiver,uint256 nonce)",
        );
        let zero = SafeTx {
            to: Address::ZERO,
            value: U256::ZERO,
            data: alloy::primitives::Bytes::new(),
            operation: 0,
            safeTxGas: U256::ZERO,
            baseGas: U256::ZERO,
            gasPrice: U256::ZERO,
            gasToken: Address::ZERO,
            refundReceiver: Address::ZERO,
            nonce: U256::ZERO,
        };
        assert_eq!(zero.eip712_type_hash(), want);
    }

    #[test]
    fn sign_safe_tx_produces_ethereum_v_byte() {
        // Round-trip: build a deterministic SafeTransaction, sign, check v is 27 or 28.
        let signer = PMCup26Signer::from_hex(
            "0x4242424242424242424242424242424242424242424242424242424242424242",
            143,
        )
        .unwrap();
        let safe = Address::ZERO; // verifyingContract for the test
        let tx = crate::safe::SafeTransaction::call(
            Address::ZERO,
            vec![0x01, 0x02, 0x03],
            U256::ZERO,
        );
        let sig = signer.sign_safe_tx(safe, &tx).unwrap();
        assert!(sig[64] == 27 || sig[64] == 28, "expected Ethereum v {{27,28}}, got {}", sig[64]);
    }
}
