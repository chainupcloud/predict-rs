//! Order construction.
//!
//! Builds a [`SignableOrder`] / [`SignedOrder`] from human-readable inputs (`price` / `size`
//! / `amount` decimal values) plus the chain-level context owned by [`PMCup26Signer`].
//!
//! The builder mirrors `rs-clob-client::OrderBuilder` API shape with one concrete
//! difference: chainup orders carry `scopeId` which comes from the signer, NOT a builder
//! field. The salt defaults to a deterministic-per-call value (current ns time, masked to
//! 53 bits per pm-sdk-go convention) so the signed order is reproducible only when the
//! caller pins it via `.salt(...)`.
//!
//! ## Amount math
//!
//! `price` in `(0, 1)`, `size` in **shares (human-readable)**, both [`Decimal`].
//! The 6-decimal USDC/CTF scaling matches `pm-sdk-go::toBaseUnits` (`Truncate(6).Shift(6)`).
//!
//! - **BUY**: `makerAmount = price × size × 10^6`, `takerAmount = size × 10^6`
//! - **SELL**: `makerAmount = size × 10^6`, `takerAmount = price × size × 10^6`
//!
//! Tick-size enforcement: when the builder has fetched the per-market `minimum_tick_size`
//! (via `--with-tick-size` or via the bundled `Client::limit_order(...)` flow that consults
//! `/tick-size`), `price` is required to have at most that many decimals and to lie inside
//! the inner interval `[tick, 1 - tick]`. `size` is capped at the chainup lot size of 2
//! decimals.
//!
//! ## Signature
//!
//! [`OrderBuilder::build_and_sign`] computes the EIP-712 digest via the shared
//! [`PMCup26Signer::sign_order`] entry-point. The 65-byte `r||s||v` output is normalised so
//! `v ∈ {27, 28}` — required by the on-chain `ECDSA.recover` path that the relayer takes
//! when settling matched trades (`pm-sdk-go::clob.normalizeECDSAv`). The server-side
//! verifier accepts both `{0,1}` and `{27,28}`; the SDK emits `{27,28}` for end-to-end
//! parity.

use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::clob::types::{OrderType, SignableOrder, SignedOrder};
use crate::error::{Error, Result};
use crate::signer::{OrderForSigning, PMCup26Signer};
use crate::types::{Address, ScopeId, Side, SignatureType, U256};

/// USDC / CTF outcome-token scale (`10^6`). Matches `services/clob-service/CLAUDE.md`
/// "Token 与 USDC 精度" and `pm-sdk-go::usdcDecimals`.
pub const USDC_DECIMALS: u32 = 6;

/// Maximum decimal places for `size` (lot size). Matches Polymarket reference.
pub const LOT_SIZE_SCALE: u32 = 2;

/// Marker for the limit-order builder variant.
#[derive(Debug, Default)]
pub struct Limit;

/// Marker for the market-order builder variant.
#[derive(Debug, Default)]
pub struct Market;

/// Type-state trait for builder variant.
pub trait OrderKind: 'static {
    const IS_MARKET: bool;
}

impl OrderKind for Limit {
    const IS_MARKET: bool = false;
}

impl OrderKind for Market {
    const IS_MARKET: bool = true;
}

/// Builder for a [`SignableOrder`]. Created via [`OrderBuilder::limit`] or
/// [`OrderBuilder::market`]; finalised via [`OrderBuilder::build`] (returns
/// [`SignableOrder`] for external signing) or [`OrderBuilder::build_and_sign`].
#[derive(Debug)]
pub struct OrderBuilder<K: OrderKind> {
    token_id: Option<U256>,
    price: Option<Decimal>,
    size: Option<Decimal>,
    /// Used only by the market-order variant. Always denominated in **shares** when
    /// `side = SELL` (server-side requirement); BUY accepts shares or USDC (USDC ⇒ shares
    /// via `amount / price`).
    amount: Option<Decimal>,
    amount_in_usdc: bool,
    side: Option<Side>,
    order_type: OrderType,
    post_only: bool,
    expiration: u64,
    nonce: u64,
    fee_rate_bps: Option<u64>,
    maker: Option<Address>,
    taker: Address,
    signature_type: SignatureType,
    salt: Option<U256>,
    owner: String,
    /// Optional per-market tick size; when present, [`Self::build`] validates the price
    /// against it. Filled by [`crate::Client::limit_order`] from `GET /tick-size`.
    minimum_tick_size: Option<Decimal>,
    _kind: PhantomData<K>,
}

impl OrderBuilder<Limit> {
    /// New limit-order builder. `OrderType` defaults to GTC.
    #[must_use]
    pub fn limit() -> Self {
        Self::default_for(OrderType::Gtc)
    }
}

impl OrderBuilder<Market> {
    /// New market-order builder. `OrderType` defaults to FAK.
    #[must_use]
    pub fn market() -> Self {
        Self::default_for(OrderType::Fak)
    }

    /// Set the market-order amount in **shares** (matches limit-order `.size()` semantics).
    #[must_use]
    pub fn shares(mut self, amount: Decimal) -> Self {
        self.amount = Some(amount);
        self.amount_in_usdc = false;
        self
    }

    /// Set the market-order amount in **USDC**. BUY only — SELL must specify shares.
    #[must_use]
    pub fn usdc(mut self, amount: Decimal) -> Self {
        self.amount = Some(amount);
        self.amount_in_usdc = true;
        self
    }
}

impl<K: OrderKind> OrderBuilder<K> {
    fn default_for(order_type: OrderType) -> Self {
        Self {
            token_id: None,
            price: None,
            size: None,
            amount: None,
            amount_in_usdc: false,
            side: None,
            order_type,
            post_only: false,
            expiration: 0,
            nonce: 0,
            fee_rate_bps: None,
            maker: None,
            taker: Address::ZERO,
            signature_type: SignatureType::PolyGnosisSafe,
            salt: None,
            owner: String::new(),
            minimum_tick_size: None,
            _kind: PhantomData,
        }
    }

    /// Token id (uint256). Required.
    #[must_use]
    pub fn token_id(mut self, token_id: impl Into<U256>) -> Self {
        self.token_id = Some(token_id.into());
        self
    }

    /// Limit price (also accepted by market orders as a fallback when book-walking is not
    /// implemented client-side — chainup performs the actual market matching server-side).
    #[must_use]
    pub fn price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Order size in shares (limit-order convenience; market orders should use `.shares()`
    /// or `.usdc()`).
    #[must_use]
    pub fn size(mut self, size: Decimal) -> Self {
        self.size = Some(size);
        self
    }

    /// Order side (BUY / SELL). Required.
    #[must_use]
    pub fn side(mut self, side: Side) -> Self {
        self.side = Some(side);
        self
    }

    /// Override the default order type. For limit orders the default is GTC; for market
    /// orders the default is FAK.
    #[must_use]
    pub fn order_type(mut self, order_type: OrderType) -> Self {
        self.order_type = order_type;
        self
    }

    /// `postOnly` flag (limit orders only).
    #[must_use]
    pub fn post_only(mut self, v: bool) -> Self {
        self.post_only = v;
        self
    }

    /// Unix-seconds expiration. Required when `order_type == GTD`; otherwise must be 0.
    #[must_use]
    pub fn expiration(mut self, ts: u64) -> Self {
        self.expiration = ts;
        self
    }

    /// Server-side rotation nonce (defaults to 0). Distinct from API-key nonce.
    #[must_use]
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Per-event fee rate in basis points. Required — the server rejects orders that fall
    /// below the configured event minimum.
    #[must_use]
    pub fn fee_rate_bps(mut self, bps: u64) -> Self {
        self.fee_rate_bps = Some(bps);
        self
    }

    /// Maker (Safe-wallet) address. **Required** for `signatureType = 2` (the chainup
    /// default); see `services/clob-service/CLAUDE.md` "Safe 钱包架构".
    #[must_use]
    pub fn maker(mut self, addr: Address) -> Self {
        self.maker = Some(addr);
        self
    }

    /// Taker address (default `Address::ZERO` = open / any taker).
    #[must_use]
    pub fn taker(mut self, addr: Address) -> Self {
        self.taker = addr;
        self
    }

    /// Override the signature type (default `POLY_GNOSIS_SAFE`).
    #[must_use]
    pub fn signature_type(mut self, t: SignatureType) -> Self {
        self.signature_type = t;
        self
    }

    /// Pin the salt for reproducible signatures (tests / golden fixtures). Defaults to the
    /// current Unix-nanosecond timestamp masked to 53 bits — matches
    /// `pm-sdk-go::time.Now().UnixNano()` and the rs-clob-client `generate_seed` convention
    /// (the server treats salt as IEEE-754 safe integer).
    #[must_use]
    pub fn salt(mut self, salt: impl Into<U256>) -> Self {
        self.salt = Some(salt.into());
        self
    }

    /// Optional `owner` UUID forwarded on the outer `SendOrder` envelope. When empty, the
    /// server uses the API-key owner.
    #[must_use]
    pub fn owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = owner.into();
        self
    }

    /// Per-market minimum tick size (e.g. `0.01` / `0.001` / `0.0001`). When supplied,
    /// `build` enforces price decimals + bounds against this value. Normally populated by
    /// [`crate::Client::limit_order`] from `GET /tick-size`.
    #[must_use]
    pub fn minimum_tick_size(mut self, tick: Decimal) -> Self {
        self.minimum_tick_size = Some(tick);
        self
    }

    /// Validate the inputs and produce a [`SignableOrder`] — does **not** sign. Callers
    /// that want the wire-ready signed order should use [`Self::build_and_sign`].
    pub fn build(self) -> Result<SignableOrder> {
        self.build_inner()
    }

    /// Build then sign — returns a [`SignedOrder`] (wire-ready JSON shape) **and** the
    /// outer envelope details (`order_type` / `post_only` / `owner`) in a [`SignableOrder`]
    /// hand-off — `Client::post_order` takes the [`SignableOrder`] companion plus the
    /// signature.
    pub fn build_and_sign(self, signer: &PMCup26Signer) -> Result<(SignableOrder, SignedOrder)> {
        let signable = self.build_inner_with_signer(signer)?;
        let sig = signer.sign_order(&signable.order)?;
        let sig_norm = normalize_ecdsa_v(sig);
        let signed = signed_order_from(&signable, &sig_norm)?;
        Ok((signable, signed))
    }

    /// Same as [`Self::build`] but pre-fills `signer.address()` as `signer` and validates
    /// that the configured maker matches the signer when `signature_type == EOA`.
    fn build_inner_with_signer(self, signer: &PMCup26Signer) -> Result<SignableOrder> {
        let mut signable = self.build_inner()?;
        // For EOA signing the maker / signer addresses must agree (a strict client-side
        // check; the server enforces the same for L2 orders).
        if matches!(signable.order.signature_type, x if x == SignatureType::Eoa as u8)
            && signable.order.maker != signer.address()
        {
            return Err(Error::validation(format!(
                "EOA order requires maker == signer address ({} != {})",
                signable.order.maker,
                signer.address(),
            )));
        }
        // signer of the EIP-712 struct is *always* the EOA.
        signable.order.signer = signer.address();
        // scope id flows from the signer (chainup-specific).
        signable.order.scope_id = signer.scope_id();
        Ok(signable)
    }

    fn build_inner(self) -> Result<SignableOrder> {
        let token_id = self
            .token_id
            .ok_or_else(|| Error::validation("OrderBuilder: token_id is required"))?;
        let side = self
            .side
            .ok_or_else(|| Error::validation("OrderBuilder: side is required"))?;
        let fee_rate_bps = self
            .fee_rate_bps
            .ok_or_else(|| Error::validation("OrderBuilder: fee_rate_bps is required"))?;
        let order_type = self.order_type;

        // Time-in-force / expiration matrix.
        if order_type == OrderType::Gtd {
            if self.expiration == 0 {
                return Err(Error::validation(
                    "OrderBuilder: GTD orders require a non-zero expiration",
                ));
            }
        } else if self.expiration != 0 {
            return Err(Error::validation(
                "OrderBuilder: only GTD orders may set a non-zero expiration",
            ));
        }
        if self.post_only && order_type.is_market() {
            return Err(Error::validation(
                "OrderBuilder: postOnly is incompatible with FAK / FOK market orders",
            ));
        }

        // Resolve maker.
        let maker = match (self.maker, self.signature_type) {
            (Some(m), _) => m,
            (None, SignatureType::PolyGnosisSafe) => {
                return Err(Error::validation(
                    "OrderBuilder: signatureType=PolyGnosisSafe requires .maker(<Safe address>)",
                ));
            }
            // Other signature types fall back to a sentinel; the caller using
            // build_and_sign with a signer will overwrite maker with signer.address() when
            // EOA, otherwise the server-side validator will catch a misuse.
            (None, _) => Address::ZERO,
        };

        // Compute (maker_amount, taker_amount).
        let (price, size) = self.resolve_price_and_size()?;
        validate_price(price)?;
        validate_size(size)?;
        if let Some(tick) = self.minimum_tick_size {
            validate_price_against_tick(price, tick)?;
        }

        let (maker_amount, taker_amount) = compute_amounts(side, price, size)?;

        let salt = self.salt.unwrap_or_else(generate_salt);

        let order = OrderForSigning {
            salt,
            maker,
            signer: maker, // overwritten by build_inner_with_signer when a signer is known
            taker: self.taker,
            token_id,
            maker_amount,
            taker_amount,
            expiration: self.expiration,
            nonce: self.nonce,
            fee_rate_bps,
            side: side.as_u8(),
            signature_type: self.signature_type.as_u8(),
            // Default zero; the build_inner_with_signer wrapper overwrites this with the
            // signer's scope id so the on-chain EIP-712 digest matches what the signer
            // would compute.
            scope_id: ScopeId::ZERO,
        };

        Ok(SignableOrder {
            order,
            order_type,
            post_only: self.post_only,
            owner: self.owner,
        })
    }

    fn resolve_price_and_size(&self) -> Result<(Decimal, Decimal)> {
        // Limit-order path.
        if !K::IS_MARKET {
            let price = self
                .price
                .ok_or_else(|| Error::validation("OrderBuilder: limit orders require .price()"))?;
            let size = self
                .size
                .ok_or_else(|| Error::validation("OrderBuilder: limit orders require .size()"))?;
            return Ok((price, size));
        }
        // Market-order path.
        let price = self.price.ok_or_else(|| {
            Error::validation(
                "OrderBuilder: market orders need a limit price (the chainup server runs the \
                 actual book walk; the signed order still carries makerAmount/takerAmount \
                 anchored at this price)",
            )
        })?;
        let amount = self.amount.ok_or_else(|| {
            Error::validation(
                "OrderBuilder: market orders require .shares(...) or .usdc(...) for the amount",
            )
        })?;
        let side = self
            .side
            .ok_or_else(|| Error::validation("OrderBuilder: side is required"))?;
        let size = if self.amount_in_usdc {
            if side == Side::Sell {
                return Err(Error::validation(
                    "OrderBuilder: SELL market orders must specify the amount in shares",
                ));
            }
            // shares = usdc / price (server walks the book; this is the anchor).
            if price.is_zero() {
                return Err(Error::validation(
                    "OrderBuilder: market price cannot be zero",
                ));
            }
            amount / price
        } else {
            amount
        };
        Ok((price, size))
    }
}

/// Compute the chainup `(makerAmount, takerAmount)` per Side.
///
/// Both outputs are 6-decimal raw integers (`u128` -> `U256`). Truncates with floor —
/// matches `pm-sdk-go::toBaseUnits` (`Truncate(6).Shift(6).Truncate(0)`).
pub(crate) fn compute_amounts(side: Side, price: Decimal, size: Decimal) -> Result<(U256, U256)> {
    let notional = price * size;
    let size_units = to_base_units(size)?;
    let notional_units = to_base_units(notional)?;
    let (maker, taker) = match side {
        Side::Buy => (notional_units, size_units),
        Side::Sell => (size_units, notional_units),
    };
    Ok((U256::from(maker), U256::from(taker)))
}

/// Truncate to 6 decimal places, shift left by `10^6`, return as `u128`. Errors when the
/// value is negative.
fn to_base_units(d: Decimal) -> Result<u128> {
    if d.is_sign_negative() {
        return Err(Error::validation(format!(
            "amount {d} cannot be negative",
        )));
    }
    let truncated = d.trunc_with_scale(USDC_DECIMALS);
    let scaled = truncated * Decimal::from(1_000_000u64);
    scaled
        .trunc()
        .to_u128()
        .ok_or_else(|| Error::validation(format!("amount {d} overflows u128 base units")))
}

fn validate_price(price: Decimal) -> Result<()> {
    if !price.is_sign_positive() || price.is_zero() {
        return Err(Error::validation(format!(
            "price must be strictly positive, got {price}"
        )));
    }
    if price >= Decimal::ONE {
        return Err(Error::validation(format!(
            "price must lie in the open interval (0, 1), got {price}"
        )));
    }
    Ok(())
}

fn validate_size(size: Decimal) -> Result<()> {
    if !size.is_sign_positive() || size.is_zero() {
        return Err(Error::validation(format!(
            "size must be strictly positive, got {size}"
        )));
    }
    if size.scale() > LOT_SIZE_SCALE {
        return Err(Error::validation(format!(
            "size {size} has {} decimals; chainup lot size is {LOT_SIZE_SCALE}",
            size.scale()
        )));
    }
    Ok(())
}

fn validate_price_against_tick(price: Decimal, tick: Decimal) -> Result<()> {
    let tick_scale = tick.scale();
    if price.scale() > tick_scale {
        return Err(Error::validation(format!(
            "price {price} has {} decimals; minimum_tick_size {tick} has {tick_scale}",
            price.scale()
        )));
    }
    let upper = Decimal::ONE - tick;
    if price < tick || price > upper {
        return Err(Error::validation(format!(
            "price {price} is outside the tick-aligned interval [{tick}, {upper}]"
        )));
    }
    Ok(())
}

/// Build a wire-ready [`SignedOrder`] from a [`SignableOrder`] and a 65-byte
/// `r||s||v`-encoded signature (with `v` already normalised to `{27, 28}`).
///
/// Most callers should use [`OrderBuilder::build_and_sign`]; this is exposed for the
/// "build then sign externally" flow (e.g. AWS-KMS / remote-signer integrations) and for
/// tests.
pub fn signed_order_from(signable: &SignableOrder, sig: &[u8; 65]) -> Result<SignedOrder> {
    let o = &signable.order;
    let mut sig_hex = String::with_capacity(2 + sig.len() * 2);
    sig_hex.push_str("0x");
    sig_hex.push_str(&hex::encode(sig));

    let scope_hex = o.scope_id.to_hex();
    Ok(SignedOrder {
        salt: o.salt.to_string(),
        maker: format_address(o.maker),
        signer: format_address(o.signer),
        taker: format_address(o.taker),
        token_id: o.token_id.to_string(),
        maker_amount: o.maker_amount.to_string(),
        taker_amount: o.taker_amount.to_string(),
        expiration: o.expiration.to_string(),
        nonce: o.nonce.to_string(),
        fee_rate_bps: o.fee_rate_bps.to_string(),
        side: match o.side {
            0 => Side::Buy,
            1 => Side::Sell,
            other => {
                return Err(Error::validation(format!(
                    "invalid side byte {other} on signed order"
                )));
            }
        },
        signature_type: o.signature_type.to_string(),
        signature: sig_hex,
        scope_id: scope_hex,
    })
}

#[must_use]
fn format_address(addr: Address) -> String {
    format!("{addr:#x}")
}

/// Take a 65-byte signature with `v ∈ {0, 1}` (Rust signer output / go-ethereum `Sign`) and
/// produce one with `v ∈ {27, 28}` — required by `OpenZeppelin/contracts ECDSA.recover`
/// invoked from `CTFExchange.matchOrders` and the Safe 1-of-1 verifier path.
///
/// Idempotent: passes through unchanged when `v >= 27`. Matches
/// `pm-sdk-go/pkg/clob/helpers.go::normalizeECDSAv`.
#[must_use]
pub fn normalize_ecdsa_v(mut sig: [u8; 65]) -> [u8; 65] {
    if sig[64] < 27 {
        sig[64] += 27;
    }
    sig
}

/// Default salt: current Unix-nanosecond timestamp masked to 53 bits. Matches
/// `pm-sdk-go::time.Now().UnixNano()` and the Polymarket `generate_seed` convention (server
/// treats `salt` as IEEE-754 integer-safe).
#[must_use]
fn generate_salt() -> U256 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Mask to 53 bits — same as rs-clob-client `to_ieee_754_int`.
    let masked = u64::try_from(nanos & ((1u128 << 53) - 1)).unwrap_or(0);
    U256::from(masked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn to_base_units_truncates() {
        assert_eq!(to_base_units(dec!(123.456)).unwrap(), 123_456_000);
        assert_eq!(to_base_units(dec!(123.456789)).unwrap(), 123_456_789);
        // Anything past 6 decimals is truncated, not rounded.
        assert_eq!(to_base_units(dec!(123.4567899)).unwrap(), 123_456_789);
        assert_eq!(to_base_units(dec!(0)).unwrap(), 0);
    }

    #[test]
    fn buy_amounts_match_chainup_formula() {
        // BUY 100 shares at $0.34 -> makerAmount=34_000_000 USDC, takerAmount=100_000_000 token.
        let (maker, taker) = compute_amounts(Side::Buy, dec!(0.34), dec!(100)).unwrap();
        assert_eq!(maker, U256::from(34_000_000u64));
        assert_eq!(taker, U256::from(100_000_000u64));
    }

    #[test]
    fn sell_amounts_match_chainup_formula() {
        // SELL 100 shares at $0.65 -> makerAmount=100_000_000 token, takerAmount=65_000_000 USDC.
        let (maker, taker) = compute_amounts(Side::Sell, dec!(0.65), dec!(100)).unwrap();
        assert_eq!(maker, U256::from(100_000_000u64));
        assert_eq!(taker, U256::from(65_000_000u64));
    }

    #[test]
    fn amounts_truncate_at_6_decimals() {
        // BUY 1 share at $0.123456789 → notional 0.123456 (truncated), size 1.
        let (maker, taker) =
            compute_amounts(Side::Buy, dec!(0.123456789), dec!(1)).unwrap();
        assert_eq!(maker, U256::from(123_456u64));
        assert_eq!(taker, U256::from(1_000_000u64));
    }

    #[test]
    fn tick_001_rejects_higher_precision_price() {
        let err = validate_price_against_tick(dec!(0.501), dec!(0.01)).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn tick_001_accepts_in_range() {
        validate_price_against_tick(dec!(0.50), dec!(0.01)).unwrap();
        validate_price_against_tick(dec!(0.99), dec!(0.01)).unwrap();
        validate_price_against_tick(dec!(0.01), dec!(0.01)).unwrap();
    }

    #[test]
    fn tick_0001_accepts_three_decimals() {
        // tick 0.001 (3 decimals) accepts ≤3-decimal prices, rejects 4-decimal.
        validate_price_against_tick(dec!(0.012), dec!(0.001)).unwrap();
        validate_price_against_tick(dec!(0.0123), dec!(0.001)).unwrap_err();
    }

    #[test]
    fn tick_00001_accepts_four_decimals() {
        // tick 0.0001 (4 decimals) accepts 4-decimal prices.
        validate_price_against_tick(dec!(0.0123), dec!(0.0001)).unwrap();
        // and rejects 5-decimal.
        validate_price_against_tick(dec!(0.01234), dec!(0.0001)).unwrap_err();
    }

    #[test]
    fn size_rejects_more_than_2_decimals() {
        validate_size(dec!(1.234)).unwrap_err();
        validate_size(dec!(1.23)).unwrap();
    }

    #[test]
    fn price_must_be_in_open_interval() {
        validate_price(dec!(0)).unwrap_err();
        validate_price(dec!(1)).unwrap_err();
        validate_price(Decimal::new(-1, 2)).unwrap_err();
        validate_price(dec!(0.5)).unwrap();
    }

    #[test]
    fn normalize_v_idempotent() {
        let mut sig = [0u8; 65];
        sig[64] = 0;
        assert_eq!(normalize_ecdsa_v(sig)[64], 27);
        sig[64] = 1;
        assert_eq!(normalize_ecdsa_v(sig)[64], 28);
        sig[64] = 27;
        assert_eq!(normalize_ecdsa_v(sig)[64], 27);
        sig[64] = 28;
        assert_eq!(normalize_ecdsa_v(sig)[64], 28);
    }

    #[test]
    fn build_limit_order_default_signature_type_requires_maker() {
        let builder = OrderBuilder::<Limit>::limit()
            .token_id(U256::from(100u64))
            .price(dec!(0.5))
            .size(dec!(10))
            .side(Side::Buy)
            .fee_rate_bps(100);
        let err = builder.build().unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn build_limit_order_emits_correct_amounts() {
        let signable = OrderBuilder::<Limit>::limit()
            .token_id(U256::from(100u64))
            .price(dec!(0.34))
            .size(dec!(100))
            .side(Side::Buy)
            .fee_rate_bps(100)
            .maker(Address::ZERO)
            .signature_type(SignatureType::Eoa)
            .build()
            .unwrap();
        assert_eq!(signable.order.maker_amount, U256::from(34_000_000u64));
        assert_eq!(signable.order.taker_amount, U256::from(100_000_000u64));
        assert_eq!(signable.order_type, OrderType::Gtc);
    }

    #[test]
    fn gtd_requires_expiration() {
        let err = OrderBuilder::<Limit>::limit()
            .token_id(U256::from(100u64))
            .price(dec!(0.5))
            .size(dec!(10))
            .side(Side::Buy)
            .fee_rate_bps(100)
            .maker(Address::ZERO)
            .signature_type(SignatureType::Eoa)
            .order_type(OrderType::Gtd)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn gtc_rejects_expiration() {
        let err = OrderBuilder::<Limit>::limit()
            .token_id(U256::from(100u64))
            .price(dec!(0.5))
            .size(dec!(10))
            .side(Side::Buy)
            .fee_rate_bps(100)
            .maker(Address::ZERO)
            .signature_type(SignatureType::Eoa)
            .expiration(1_700_000_000)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn build_market_order_with_usdc_amount() {
        let signable = OrderBuilder::<Market>::market()
            .token_id(U256::from(100u64))
            .price(dec!(0.34))
            .side(Side::Buy)
            .usdc(dec!(34))
            .fee_rate_bps(100)
            .maker(Address::ZERO)
            .signature_type(SignatureType::Eoa)
            .build()
            .unwrap();
        // USDC 34 / 0.34 = 100 shares -> makerAmount = 34_000_000, takerAmount = 100_000_000.
        assert_eq!(signable.order.maker_amount, U256::from(34_000_000u64));
        assert_eq!(signable.order.taker_amount, U256::from(100_000_000u64));
        assert_eq!(signable.order_type, OrderType::Fak);
    }

    #[test]
    fn build_market_sell_with_usdc_amount_errors() {
        let err = OrderBuilder::<Market>::market()
            .token_id(U256::from(100u64))
            .price(dec!(0.34))
            .side(Side::Sell)
            .usdc(dec!(34))
            .fee_rate_bps(100)
            .maker(Address::ZERO)
            .signature_type(SignatureType::Eoa)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }
}
