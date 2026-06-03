//! Builder → sign → JSON wire round-trip.
//!
//! Reuses the first order in `tests/fixtures/golden.json` (a vector confirmed against
//! `pm-sdk-go::signer`): `chain137_buy_scope42`.
//!
//! Asserts:
//!
//! 1. Hand-crafting an `OrderBuilder` with the same maker / taker / token / fee / salt /
//!    scope / signer as the golden vector and calling `.build_and_sign(...)` produces a
//!    `SignedOrder` whose 65-byte signature decodes back to the golden hex bytes (after
//!    stripping the +27 normalisation).
//! 2. The `SignedOrder` JSON serialises to the exact field set documented in openapi.yaml
//!    (`tokenID` / `makerAmount` / `feeRateBps` mixed-case, side `"BUY"`, signatureType
//!    `"0"`, scopeId 0x-prefixed 64 hex chars).
//! 3. Round-tripping the JSON through `serde` recovers an equal `SignedOrder`.

use predict_rs_clob_client::clob::order_builder::{normalize_ecdsa_v, signed_order_from};
use predict_rs_clob_client::clob::types::{OrderType, SignableOrder, SignedOrder};
use predict_rs_clob_client::signer::OrderForSigning;
use predict_rs_clob_client::types::{Address, ScopeId, SignatureType, U256, Side};
use predict_rs_clob_client::PMCup26Signer;

const PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const CHAIN_ID: u64 = 137;
const EXCHANGE: &str = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E";
const SCOPE_HEX: &str = "0x0000000000000000000000000000000000000000000000000000000000000042";
/// Matches the first golden order vector signature (with v ∈ {0,1}). The SDK emits +27.
const GOLDEN_SIG_RAW: &str = "0x9ce18e333eab863df79594d6aef05c540f8a4cceac1962a03cf4a2294286919f040527b6c683d61f5a2cfce7b0d0d468ef88d975ee219aa8d4566e64409e0e4a00";

fn signable_from_golden_vector() -> SignableOrder {
    SignableOrder {
        order: OrderForSigning {
            salt: U256::from(12345u64),
            maker: "0x0000000000000000000000000000000000000001".parse().unwrap(),
            signer: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".parse().unwrap(),
            taker: Address::ZERO,
            token_id: U256::from(100u64),
            maker_amount: U256::from(500_000u64),
            taker_amount: U256::from(1_000_000u64),
            expiration: 0,
            nonce: 1,
            fee_rate_bps: 100,
            side: 0, // BUY
            signature_type: 0, // EOA
            scope_id: ScopeId::from_hex(SCOPE_HEX).unwrap(),
        },
        order_type: OrderType::Gtc,
        post_only: false,
        owner: String::new(),
    }
}

#[test]
fn signed_order_matches_golden_vector() {
    let signable = signable_from_golden_vector();
    let signer = PMCup26Signer::from_hex(PRIVATE_KEY, CHAIN_ID)
        .unwrap()
        .with_scope_id(signable.order.scope_id)
        .with_exchange(EXCHANGE.parse().unwrap());
    let raw_sig = signer.sign_order(&signable.order).unwrap();
    // golden_signer.rs already proved this matches GOLDEN_SIG_RAW byte-for-byte; assert it
    // here too so a future regression points at this round-trip test instead of just the
    // generic golden_signer suite.
    let golden_bytes = hex::decode(GOLDEN_SIG_RAW.trim_start_matches("0x")).unwrap();
    assert_eq!(raw_sig.as_slice(), golden_bytes.as_slice());

    // Now the SDK's `+27` normalisation.
    let normalised = normalize_ecdsa_v(raw_sig);
    assert_eq!(normalised[64], golden_bytes[64] + 27);

    let signed = signed_order_from(&signable, &normalised).unwrap();
    // Field-by-field assertions (mixed case for tokenID / makerAmount / etc.).
    assert_eq!(signed.salt, "12345");
    assert_eq!(signed.token_id, "100");
    assert_eq!(signed.maker_amount, "500000");
    assert_eq!(signed.taker_amount, "1000000");
    assert_eq!(signed.nonce, "1");
    assert_eq!(signed.expiration, "0");
    assert_eq!(signed.fee_rate_bps, "100");
    assert_eq!(signed.side, Side::Buy);
    assert_eq!(signed.signature_type, "0");
    assert_eq!(signed.scope_id, SCOPE_HEX);
    assert!(signed.signature.starts_with("0x"));
    assert_eq!(signed.signature.len(), 2 + 130);
    // Signature ends with the normalised v byte (golden v=0 + 27 = 0x1b).
    assert!(
        signed.signature.ends_with("1b"),
        "signature should end with normalised v=0x1b, got {}",
        &signed.signature[signed.signature.len() - 4..]
    );
    assert_eq!(signed.signature_type_enum(), Some(SignatureType::Eoa));
}

#[test]
fn signed_order_json_shape() {
    let signable = signable_from_golden_vector();
    let signer = PMCup26Signer::from_hex(PRIVATE_KEY, CHAIN_ID)
        .unwrap()
        .with_scope_id(signable.order.scope_id)
        .with_exchange(EXCHANGE.parse().unwrap());
    let raw_sig = signer.sign_order(&signable.order).unwrap();
    let normalised = normalize_ecdsa_v(raw_sig);
    let signed = signed_order_from(&signable, &normalised).unwrap();

    let json = serde_json::to_value(&signed).unwrap();
    // Cross-check exact field names — these are what `handlers.orderJSON` parses.
    let expected_keys: std::collections::BTreeSet<&str> = [
        "salt",
        "maker",
        "signer",
        "taker",
        "tokenID",
        "makerAmount",
        "takerAmount",
        "expiration",
        "nonce",
        "feeRateBps",
        "side",
        "signatureType",
        "signature",
        "scopeId",
    ]
    .into_iter()
    .collect();
    let actual_keys: std::collections::BTreeSet<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual_keys, expected_keys, "unexpected JSON keys");
    assert_eq!(json["side"], "BUY");
    assert_eq!(json["signatureType"], "0");
    assert_eq!(json["scopeId"], SCOPE_HEX);
}

#[test]
fn signed_order_omits_scope_when_empty() {
    let mut signable = signable_from_golden_vector();
    signable.order.scope_id = ScopeId::ZERO;
    let mut sig = [0u8; 65];
    sig[64] = 27;
    let signed = signed_order_from(&signable, &sig).unwrap();
    let json = serde_json::to_value(&signed).unwrap();
    assert!(
        json.get("scopeId").is_none(),
        "scopeId should be omitted when zero; got {json}"
    );
}

#[test]
fn signed_order_json_round_trip() {
    let signable = signable_from_golden_vector();
    let signer = PMCup26Signer::from_hex(PRIVATE_KEY, CHAIN_ID)
        .unwrap()
        .with_scope_id(signable.order.scope_id)
        .with_exchange(EXCHANGE.parse().unwrap());
    let raw_sig = signer.sign_order(&signable.order).unwrap();
    let normalised = normalize_ecdsa_v(raw_sig);
    let signed = signed_order_from(&signable, &normalised).unwrap();
    let json_str = serde_json::to_string(&signed).unwrap();
    let parsed: SignedOrder = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, signed, "round-trip JSON differs from original");
}

#[test]
fn buy_amounts_via_builder_match_formula() {
    use predict_rs_clob_client::clob::order_builder::OrderBuilder;
    use predict_rs_clob_client::clob::order_builder::Limit;
    use rust_decimal_macros::dec;
    // BUY 100 @ 0.34 produces makerAmount = 34_000_000 (USDC), takerAmount = 100_000_000 (tok).
    let signable = OrderBuilder::<Limit>::limit()
        .token_id(U256::from(100u64))
        .price(dec!(0.34))
        .size(dec!(100))
        .side(Side::Buy)
        .fee_rate_bps(100)
        .maker(Address::ZERO)
        .signature_type(SignatureType::Eoa)
        .salt(U256::from(7u64))
        .build()
        .unwrap();
    assert_eq!(signable.order.maker_amount, U256::from(34_000_000u64));
    assert_eq!(signable.order.taker_amount, U256::from(100_000_000u64));
    assert_eq!(signable.order.salt, U256::from(7u64));
}

#[test]
fn sell_amounts_via_builder_match_formula() {
    use predict_rs_clob_client::clob::order_builder::OrderBuilder;
    use predict_rs_clob_client::clob::order_builder::Limit;
    use rust_decimal_macros::dec;
    let signable = OrderBuilder::<Limit>::limit()
        .token_id(U256::from(100u64))
        .price(dec!(0.65))
        .size(dec!(100))
        .side(Side::Sell)
        .fee_rate_bps(100)
        .maker(Address::ZERO)
        .signature_type(SignatureType::Eoa)
        .salt(U256::from(8u64))
        .build()
        .unwrap();
    // SELL: makerAmount in tokens, takerAmount in USDC.
    assert_eq!(signable.order.maker_amount, U256::from(100_000_000u64));
    assert_eq!(signable.order.taker_amount, U256::from(65_000_000u64));
}

#[test]
fn build_and_sign_via_builder_normalises_v() {
    use predict_rs_clob_client::clob::order_builder::OrderBuilder;
    use predict_rs_clob_client::clob::order_builder::Limit;
    use rust_decimal_macros::dec;
    let signer = PMCup26Signer::from_hex(PRIVATE_KEY, CHAIN_ID)
        .unwrap()
        .with_scope_id(ScopeId::from_hex(SCOPE_HEX).unwrap())
        .with_exchange(EXCHANGE.parse().unwrap());
    let (_signable, signed) = OrderBuilder::<Limit>::limit()
        .token_id(U256::from(100u64))
        .price(dec!(0.34))
        .size(dec!(100))
        .side(Side::Buy)
        .fee_rate_bps(100)
        .maker(signer.address())
        .signature_type(SignatureType::Eoa)
        .salt(U256::from(123u64))
        .build_and_sign(&signer)
        .unwrap();
    // Last byte of signature is the +27 normalised v ∈ {0x1b, 0x1c}.
    let last_byte_hex = &signed.signature[signed.signature.len() - 2..];
    assert!(
        last_byte_hex == "1b" || last_byte_hex == "1c",
        "v should be {{0x1b, 0x1c}} after normalisation, got 0x{last_byte_hex}"
    );
}
