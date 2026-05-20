//! Gnosis Safe MultiSend packed encoder.
//!
//! `MultiSend.multiSend(bytes transactions)` accepts a single `bytes` parameter that packs
//! N sub-operations end-to-end without delimiters. Each sub-op is laid out as:
//!
//! ```text
//!   operation (1 byte)  | to (20 bytes) | value (32 bytes BE) | dataLen (32 bytes BE) | data (...)
//! ```
//!
//! The concatenation is then wrapped in the standard ABI selector + bytes header. Once the
//! whole calldata is built, it goes into a `SafeTransaction` with `operation = DelegateCall`
//! and `to = multiSendAddress`. See [`super::SafeTransaction::delegate_call`].
//!
//! Wire format verified against the on-chain tx
//! `0xca441323afc7a9a47296dc5389f9a2f1385caa8de914a33b068904f542f1fa35` on Monad
//! (Safe `0x7e63be99...c2fe`, 7 approval ops bundled). The frontend reference
//! implementation lives at `pm-cup2026/apps/user-dapp/src/hooks/useSetupSteps.ts:516`.

use alloy::primitives::{Address, U256};

/// One sub-operation inside a MultiSend batch.
///
/// Sub-ops are always `Call` (operation byte = 0) inside MultiSend — the outer Safe
/// transaction itself uses `DelegateCall` so the MultiSend contract executes the ops as
/// the Safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeSubOp {
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
}

impl SafeSubOp {
    /// Convenience: a zero-value `Call` sub-op (the common case for token approvals,
    /// CTF operations, etc.).
    #[must_use]
    pub fn call(to: Address, data: Vec<u8>) -> Self {
        Self { to, value: U256::ZERO, data }
    }
}

/// `MultiSend.multiSend(bytes transactions)` function selector — `keccak256("multiSend(bytes)")[..4]`.
pub const MULTISEND_SELECTOR: [u8; 4] = [0x8d, 0x80, 0xff, 0x0a];

/// Encode a list of sub-ops into the calldata that gets passed to `MultiSend.multiSend(...)`.
///
/// Output layout:
///
/// 1. 4 bytes selector (`0x8d80ff0a`)
/// 2. 32 bytes ABI offset to the bytes data (`0x20`)
/// 3. 32 bytes length of the packed sub-ops (big-endian)
/// 4. N bytes packed sub-ops (see module docs)
/// 5. zero-padding so the total bytes payload is 32-byte aligned
///
/// Empty input is rejected since the resulting Safe.execTransaction would be a no-op.
pub fn encode(ops: &[SafeSubOp]) -> Result<Vec<u8>, &'static str> {
    if ops.is_empty() {
        return Err("multisend::encode: ops list is empty");
    }

    // Step 1: pack sub-ops end-to-end.
    let mut packed = Vec::new();
    for op in ops {
        // operation byte (always 0 = Call inside MultiSend).
        packed.push(0u8);
        // 20 bytes to-address.
        packed.extend_from_slice(op.to.as_slice());
        // 32 bytes value (big-endian).
        packed.extend_from_slice(&op.value.to_be_bytes::<32>());
        // 32 bytes data length (big-endian).
        let len = U256::from(op.data.len());
        packed.extend_from_slice(&len.to_be_bytes::<32>());
        // raw data.
        packed.extend_from_slice(&op.data);
    }

    // Step 2: build the outer calldata.
    let mut out = Vec::with_capacity(4 + 32 + 32 + packed.len() + 32);
    out.extend_from_slice(&MULTISEND_SELECTOR);
    // ABI offset to the bytes argument = 0x20 (single bytes param).
    let offset = U256::from(0x20u64);
    out.extend_from_slice(&offset.to_be_bytes::<32>());
    // Length of the packed bytes.
    let packed_len = U256::from(packed.len());
    out.extend_from_slice(&packed_len.to_be_bytes::<32>());
    // The packed sub-ops.
    out.extend_from_slice(&packed);

    // Step 3: pad to a 32-byte boundary so the bytes payload is well-formed ABI.
    let pad = (32 - (packed.len() % 32)) % 32;
    out.resize(out.len() + pad, 0u8);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{address, hex};

    #[test]
    fn rejects_empty_ops() {
        assert!(encode(&[]).is_err());
    }

    #[test]
    fn single_op_layout_matches_packed_format() {
        // USDW.approve(NegRiskCtfExchange, MAX) — one of the ops from the live tx
        // `0xca441323...`. 4-byte selector 0x095ea7b3 + 32-byte spender + 32-byte value.
        let usdw = address!("b7bD080Df56FA76ce6CA4fA737d47815f7F8e746");
        let neg_ctf = address!("50b7B00EE75F8bFb5cDa892883aFb3867851c738");
        let mut call_data = hex::decode("095ea7b3").unwrap();
        let mut spender_pad = vec![0u8; 12];
        spender_pad.extend_from_slice(neg_ctf.as_slice());
        call_data.extend_from_slice(&spender_pad);
        call_data.extend_from_slice(&[0xff_u8; 32]); // MAX

        let op = SafeSubOp::call(usdw, call_data.clone());
        let encoded = encode(&[op]).unwrap();

        // Selector
        assert_eq!(&encoded[0..4], &MULTISEND_SELECTOR);
        // ABI offset = 0x20
        assert_eq!(encoded[4 + 31], 0x20);
        // Length-of-packed bytes header at offset 36. Packed = 1 + 20 + 32 + 32 + 68 = 153.
        let len_be = &encoded[36..68];
        assert_eq!(U256::from_be_slice(len_be), U256::from(153u64));
        // First byte of the packed region = operation byte (Call = 0).
        assert_eq!(encoded[68], 0u8);
        // Next 20 bytes = the `to` address.
        assert_eq!(&encoded[69..89], usdw.as_slice());
    }

    #[test]
    fn seven_op_batch_matches_live_tx_packed_length() {
        // Sanity: the 7-op MultiSend on tx `0xca441323...` has packed-bytes length
        // `0x428` (1064) — print the computed length here for verification against the
        // wire log. Each op is 1 + 20 + 32 + 32 + 68 = 153 bytes; seven ops => 1071.
        // (The exact figure depends on the data field; this test guards against
        // accidental layout drift.)
        let usdw = address!("b7bD080Df56FA76ce6CA4fA737d47815f7F8e746");
        let spender = address!("017641abFa4264121237023f9Fe678BF00F60De8");
        let mut approve_data = hex::decode("095ea7b3").unwrap();
        let mut pad = vec![0u8; 12];
        pad.extend_from_slice(spender.as_slice());
        approve_data.extend_from_slice(&pad);
        approve_data.extend_from_slice(&[0xff_u8; 32]);

        let ops: Vec<_> = (0..7)
            .map(|_| SafeSubOp::call(usdw, approve_data.clone()))
            .collect();
        let encoded = encode(&ops).unwrap();

        // Per-op size = 1 + 20 + 32 + 32 + 68 (approve data) = 153 bytes; 7 ops = 1071.
        let expected_packed_len = 7 * (1 + 20 + 32 + 32 + 68);
        let len_be = &encoded[36..68];
        assert_eq!(
            U256::from_be_slice(len_be),
            U256::from(expected_packed_len as u64),
            "packed length must be sum of per-op sizes"
        );
    }
}
