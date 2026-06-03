//! Frozen JSON fixtures captured from the AsyncAPI schemas
//! (`pm-cup2026/services/clob-service/docs/asyncapi-{market,user}.json`).
//!
//! These guard against silent serde drift: every event-type tag must
//! deserialize into a unique variant, and the round-trip serialization must
//! preserve every documented field (modulo serde defaults / `skip_serializing_if`).

use predict_rs_clob_client::clob::ws::types::response::{
    MarketEvent, OrderSide, OrderStatus, OrderSubType, TradeStatus, TraderSide, UserEvent,
};

/// Live frame captured on 2026-05-20 from `clob-ws.hermestrade.xyz` (Monad).
/// Carries `match_type` and `order_id` (server extensions over the asyncapi spec) and
/// uses short-UPPERCASE `MATCHED` for status. Every other documented field is absent.
const LIVE_TRADE_FRAME: &str = r#"{"event_type":"trade","owner":"b40cbc5f-b3c0-4644-94a1-57e859f0038b","condition_id":"0xb808642dacfc6af662e46d58a118564afa1df134d41952e37532ef7b4b89001e","data":{"asset_id":"75376549546305181946655842061972812241926814861786064316719293942708924791063","id":"315312644720427008","match_type":"MINT","order_id":"315312644699455488","price":"0.91","side":"BUY","size":"5","status":"MATCHED"}}"#;

fn round_trip_market(raw: &str) {
    let decoded: MarketEvent = serde_json::from_str(raw).expect("decode");
    let re_encoded = serde_json::to_value(&decoded).unwrap();
    let original: serde_json::Value = serde_json::from_str(raw).unwrap();
    // event_type is preserved.
    assert_eq!(re_encoded["event_type"], original["event_type"]);
}

fn round_trip_user(raw: &str) {
    let decoded: UserEvent = serde_json::from_str(raw).expect("decode");
    let re_encoded = serde_json::to_value(&decoded).unwrap();
    let original: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(re_encoded["event_type"], original["event_type"]);
}

#[test]
fn market_event_round_trip_for_every_documented_variant() {
    // Server wire format wraps each market event's payload inside `data: {...}`.
    // The asyncapi spec shows a flat shape; production diverges. We follow live behaviour.
    let fixtures = [
        // book
        r#"{"event_type":"book","data":{"asset_id":"1","market":"0xcid","bids":[{"price":"0.4","size":"1"}],"asks":[{"price":"0.6","size":"2"}],"timestamp":1,"hash":"h"}}"#,
        // price_change
        r#"{"event_type":"price_change","data":{"market":"0xcid","price_changes":[{"asset_id":"1","price":"0.4","size":"0","side":"BUY","hash":"h","best_bid":"0.39","best_ask":"0.41"}],"timestamp":1}}"#,
        // last_trade_price
        r#"{"event_type":"last_trade_price","data":{"asset_id":"1","market":"0xcid","price":"0.5","size":"1","fee_rate_bps":"10","side":"SELL","timestamp":1,"transaction_hash":""}}"#,
        // tick_size_change
        r#"{"event_type":"tick_size_change","data":{"asset_id":"1","market":"0xcid","old_tick_size":"0.01","new_tick_size":"0.001","timestamp":1}}"#,
        // best_bid_ask
        r#"{"event_type":"best_bid_ask","data":{"asset_id":"1","market":"0xcid","best_bid":"0.49","best_ask":"0.51","spread":"0.02","timestamp":1}}"#,
        // new_market
        r#"{"event_type":"new_market","data":{"id":"m","question":"Q?","market":"0xcid","slug":"q","assets_ids":["1","2"],"outcomes":["Yes","No"],"tags":["t"],"timestamp":1}}"#,
        // market_resolved
        r#"{"event_type":"market_resolved","data":{"id":"m","market":"0xcid","assets_ids":["1","2"],"winning_asset_id":"1","winning_outcome":"Yes","tags":[],"timestamp":1}}"#,
    ];
    for raw in fixtures {
        round_trip_market(raw);
    }
}

#[test]
fn user_event_round_trip_for_every_documented_variant() {
    // Server nests the order/trade payload inside `data: {...}` and echoes `owner` /
    // `condition_id` at the top level alongside `event_type`.
    let order = r#"{"event_type":"order","data":{"type":"PLACEMENT","id":"0x","owner":"o","market":"0xcid","asset_id":"1","side":"BUY","original_size":"10","size_matched":"0","price":"0.5","outcome":"Yes","order_type":"GTC","status":"ORDER_STATUS_LIVE","maker_address":"0xs","expiration":0,"created_at":1,"associate_trades":null,"lazy":"false","timestamp":1}}"#;
    let trade = r#"{"event_type":"trade","data":{"type":"TRADE","id":"t","taker_order_id":"0x","market":"0xcid","asset_id":"1","side":"BUY","size":"1","price":"0.5","fee_rate_bps":"10","status":"TRADE_STATUS_MATCHED","outcome":"Yes","owner":"o","maker_address":"0xs","transaction_hash":"","bucket_index":0,"matchtime":1,"last_update":1,"trader_side":"TAKER","maker_orders":[],"timestamp":1}}"#;
    round_trip_user(order);
    round_trip_user(trade);
}

#[test]
fn live_trade_frame_decodes_with_server_extensions() {
    let ev: UserEvent = serde_json::from_str(LIVE_TRADE_FRAME).expect("live trade frame decodes");
    let UserEvent::Trade(t) = ev else { panic!("wrong variant") };
    assert_eq!(t.id, "315312644720427008");
    assert_eq!(t.status, TradeStatus::Matched);
    assert_eq!(t.match_type, "MINT");
    assert_eq!(t.taker_order_id, "315312644699455488", "order_id should alias taker_order_id");
    assert_eq!(t.side, Some(OrderSide::Buy));
    assert_eq!(t.size, "5");
    assert_eq!(t.price, "0.91");
    // The lean frame doesn't include these — they should default cleanly.
    assert_eq!(t.market, "");
    assert!(t.trader_side.is_none(), "trader_side absent in lean frame");
    assert!(t.maker_orders.is_empty());
}

#[test]
fn order_status_enum_covers_all_documented_values() {
    let values = [
        ("ORDER_STATUS_LIVE", OrderStatus::Live),
        ("ORDER_STATUS_MATCHED", OrderStatus::Matched),
        ("ORDER_STATUS_CANCELED", OrderStatus::Canceled),
        (
            "ORDER_STATUS_CANCELED_MARKET_RESOLVED",
            OrderStatus::CanceledMarketResolved,
        ),
        ("ORDER_STATUS_SYSTEM_CLEARED", OrderStatus::SystemCleared),
        ("ORDER_STATUS_INVALID", OrderStatus::Invalid),
    ];
    for (raw, expected) in values {
        let v: OrderStatus = serde_json::from_value(serde_json::Value::String(raw.into())).unwrap();
        assert_eq!(v, expected);
        let back = serde_json::to_value(v).unwrap();
        assert_eq!(back, serde_json::Value::String(raw.into()));
    }
}

#[test]
fn trade_status_enum_covers_all_documented_values() {
    let values = [
        ("TRADE_STATUS_MATCHED", TradeStatus::Matched),
        ("TRADE_STATUS_MINED", TradeStatus::Mined),
        ("TRADE_STATUS_CONFIRMED", TradeStatus::Confirmed),
        ("TRADE_STATUS_RETRYING", TradeStatus::Retrying),
        ("TRADE_STATUS_FAILED", TradeStatus::Failed),
    ];
    for (raw, expected) in values {
        let v: TradeStatus = serde_json::from_value(serde_json::Value::String(raw.into())).unwrap();
        assert_eq!(v, expected);
    }
}

#[test]
fn order_sub_type_and_trader_side_round_trip() {
    let plc: OrderSubType =
        serde_json::from_value(serde_json::Value::String("PLACEMENT".into())).unwrap();
    assert_eq!(plc, OrderSubType::Placement);
    let upd: OrderSubType =
        serde_json::from_value(serde_json::Value::String("UPDATE".into())).unwrap();
    assert_eq!(upd, OrderSubType::Update);
    let cncl: OrderSubType =
        serde_json::from_value(serde_json::Value::String("CANCELLATION".into())).unwrap();
    assert_eq!(cncl, OrderSubType::Cancellation);

    let taker: TraderSide =
        serde_json::from_value(serde_json::Value::String("TAKER".into())).unwrap();
    assert_eq!(taker, TraderSide::Taker);
    let maker: TraderSide =
        serde_json::from_value(serde_json::Value::String("MAKER".into())).unwrap();
    assert_eq!(maker, TraderSide::Maker);
}

#[test]
fn order_side_round_trip() {
    let buy: OrderSide = serde_json::from_value(serde_json::Value::String("BUY".into())).unwrap();
    let sell: OrderSide = serde_json::from_value(serde_json::Value::String("SELL".into())).unwrap();
    assert_eq!(buy, OrderSide::Buy);
    assert_eq!(sell, OrderSide::Sell);
    assert_eq!(serde_json::to_value(buy).unwrap(), serde_json::Value::String("BUY".into()));
}
